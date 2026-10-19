//! The MCP server inside the running app, so Claude Desktop, Claude Code and other MCP clients can
//! drive Qiyas: the tool catalogue over the Streamable HTTP transport on `127.0.0.1`, behind a
//! bearer token. Tools run on the shared session marked `Source::Mcp`.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use qiyas_core::{Session, Source};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::host::{run_host, DrawingHost, NO_HOST};
use crate::provider::png_base64;
use crate::{catalogue, execute, AiError, ToolError, ToolOutput};

/// Called after a tool changed the take-off or the BOQ work, so the window refreshes.
pub type OnMutated = Arc<dyn Fn() + Send + Sync>;
/// Called with a tool result's `ui` (another sheet, other hidden layers, a zoom), so the window
/// shows what the session now shows.
pub type OnUi = Arc<dyn Fn(Value) + Send + Sync>;

/// What a tool call answers when no drawing is open.
pub const NO_DRAWING: &str = "No drawing is open";

/// How long `serve` keeps trying a port still held by the server it replaces.
const BIND_RETRIES: u32 = 20;
const BIND_WAIT: Duration = Duration::from_millis(50);

/// Serves the tools at `http://127.0.0.1:<port>/mcp` until `shutdown` is cancelled. Every request
/// must carry `Authorization: Bearer <token>`; others get 401. Port 0 picks a free port. Gives
/// the address bound; fails when the port can't be bound. `host` runs the file tools (none:
/// they answer `NO_HOST`); a path the client gives is never counted as one the user wrote.
pub async fn serve(
    session: Arc<Mutex<Option<Session>>>,
    host: Option<Arc<dyn DrawingHost>>,
    port: u16,
    token: String,
    shutdown: CancellationToken,
    on_mutated: OnMutated,
    on_ui: OnUi,
) -> Result<SocketAddr, AiError> {
    let listener = bind(port).await?;
    let addr = listener
        .local_addr()
        .map_err(|e| AiError::Server(format!("127.0.0.1:{port}: {e}")))?;

    let handler = Qiyas::new(session, host, on_mutated, on_ui);
    // Browsers send `Origin`; only the server's own origin may call it, so a web page can't drive
    // Qiyas even with the token. MCP apps and the bridge send no `Origin` and pass.
    let port = addr.port();
    let config = StreamableHttpServerConfig::default()
        .with_cancellation_token(shutdown.child_token())
        .with_allowed_hosts(["127.0.0.1", "localhost"])
        .with_allowed_origins([
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
        ])
        .enforce_origin_validation();
    let mcp = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let app = Router::new()
        .route_service("/mcp", mcp)
        .layer(middleware::from_fn_with_state(
            Arc::new(token),
            require_token,
        ));

    tokio::spawn(async move {
        let stopped = shutdown.cancelled_owned();
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(stopped)
            .await
        {
            eprintln!("The MCP server stopped: {e}");
        }
    });
    Ok(addr)
}

/// Binds `127.0.0.1:port`, waiting a little while the server being replaced lets the port go.
async fn bind(port: u16) -> Result<tokio::net::TcpListener, AiError> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut tries = 0;
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && tries < BIND_RETRIES => {
                tries += 1;
                tokio::time::sleep(BIND_WAIT).await;
            }
            Err(e) => return Err(AiError::Server(format!("{addr}: {e}"))),
        }
    }
}

/// Lets a request through only with `Authorization: Bearer <token>`.
async fn require_token(State(token): State<Arc<String>>, req: Request, next: Next) -> Response {
    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .map(HeaderValue::as_bytes)
        .unwrap_or_default();
    let expected = format!("Bearer {token}");
    if same(given, expected.as_bytes()) {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "Missing or wrong MCP token",
        )
            .into_response()
    }
}

/// Compares in time that depends only on the lengths, so the token can't be guessed byte by byte.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The MCP handler: the catalogue's tools on the shared session.
#[derive(Clone)]
struct Qiyas {
    session: Arc<Mutex<Option<Session>>>,
    host: Option<Arc<dyn DrawingHost>>,
    on_mutated: OnMutated,
    on_ui: OnUi,
    tools: Arc<Vec<Tool>>,
    /// The tools that change the take-off or the BOQ work.
    mutating: Arc<Vec<&'static str>>,
}

impl Qiyas {
    fn new(
        session: Arc<Mutex<Option<Session>>>,
        host: Option<Arc<dyn DrawingHost>>,
        on_mutated: OnMutated,
        on_ui: OnUi,
    ) -> Qiyas {
        let specs = catalogue();
        let mutating = specs
            .iter()
            .filter(|s| s.mutating)
            .map(|s| s.name)
            .collect();
        let tools = specs
            .into_iter()
            .map(|spec| {
                let schema = match spec.input_schema {
                    Value::Object(map) => map,
                    _ => JsonObject::new(),
                };
                Tool::new(spec.name, spec.description, schema)
            })
            .collect();
        Qiyas {
            session,
            host,
            on_mutated,
            on_ui,
            tools: Arc::new(tools),
            mutating: Arc::new(mutating),
        }
    }

    /// Runs one tool on the session, holding the lock only while it runs; a file tool through the
    /// host, without the session's lock (the host locks the app's tabs first).
    fn run(&self, name: &str, input: &Value) -> Result<ToolOutput, ToolError> {
        if crate::is_window_tool(name) {
            let host = self
                .host
                .as_deref()
                .ok_or(ToolError::Failed(crate::host::NO_WINDOW.into()))?;
            return crate::run_window(host, &self.session, name, input, crate::prompt::Lang::En)
                .map_err(ToolError::Failed);
        }
        if crate::is_host_tool(name) {
            let host = self
                .host
                .as_deref()
                .ok_or(ToolError::Failed(NO_HOST.into()))?;
            return run_host(host, name, input, None)
                .map(ToolOutput::Json)
                .map_err(ToolError::Failed);
        }
        let mut guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        let session = guard
            .as_mut()
            .ok_or_else(|| ToolError::Failed(NO_DRAWING.into()))?;
        execute(session, name, input, Source::Mcp)
    }
}

impl ServerHandler for Qiyas {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("qiyas", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Qiyas is a measurement CAD for quantity surveyors. These tools read the drawing \
                 open in Qiyas and add measurements to its take-off; the user sees each change \
                 in the app and can undo it. list_drawings, new_drawing, open_drawing and \
                 switch_drawing choose the drawing the others work on; opening a file the user \
                 has not opened before, Save As and closing unsaved work are asked in the app.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.to_vec()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let input = request.arguments.map_or(Value::Null, Value::Object);
        let mutating = self.mutating.contains(&name.as_str());
        // Tools are synchronous and a picture takes a while to draw: off the async workers.
        let this = self.clone();
        let tool = name.clone();
        let output = tokio::task::spawn_blocking(move || this.run(&tool, &input))
            .await
            .map_err(|e| {
                ErrorData::internal_error(format!("The tool {name} crashed: {e}"), None)
            })?;
        let mut ui = None;
        let result = match output {
            Ok(ToolOutput::Json(value)) => {
                ui = value.get("ui").cloned();
                CallToolResult::success(vec![ContentBlock::text(value.to_string())])
            }
            Ok(ToolOutput::Image { png, caption }) => CallToolResult::success(vec![
                ContentBlock::image(png_base64(&png), "image/png"),
                ContentBlock::text(caption),
            ]),
            Err(e) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(e.to_string())]).into())
            }
        };
        if mutating {
            (self.on_mutated)();
        }
        if let Some(ui) = ui {
            (self.on_ui)(ui);
        }
        Ok(result.into())
    }
}

#[cfg(test)]
mod tests {
    use super::same;

    #[test]
    fn same_compares_whole_values() {
        assert!(same(b"Bearer abc", b"Bearer abc"));
        assert!(!same(b"Bearer abd", b"Bearer abc"));
        assert!(!same(b"Bearer ab", b"Bearer abc"));
        assert!(!same(b"", b"Bearer abc"));
    }
}
