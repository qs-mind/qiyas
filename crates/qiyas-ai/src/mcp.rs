//! The MCP server inside the running app, so Claude Desktop, Claude Code and other MCP clients can
//! drive Qiyas: the tool catalogue over the Streamable HTTP transport on `127.0.0.1`, behind a
//! bearer token. Tools run on the shared session marked `Source::Mcp`.

use std::collections::VecDeque;
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
use crate::surface::{self, Route};
use crate::{catalogue, AiError, ToolError, ToolOutput};

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
    let handler = Qiyas::new(session, host, on_mutated, on_ui);
    serve_handler(handler, port, token, shutdown).await
}

/// Mizan's local CLI connection attributes edits to the assistant, so the same Undo works.
/// Its private token cannot reach inference or any other MCP server.
pub(crate) async fn serve_agent(
    session: Arc<Mutex<Option<Session>>>,
    host: Option<Arc<dyn DrawingHost>>,
    user_text: String,
    token: String,
    shutdown: CancellationToken,
    on_ui: OnUi,
) -> Result<SocketAddr, AiError> {
    let mut handler = Qiyas::new(session, host, Arc::new(|| {}), on_ui);
    handler.source = Source::Ai;
    handler.user_text = Some(user_text);
    serve_handler(handler, 0, token, shutdown).await
}

/// Desktop connections bind once; an explicit workspace routes to an independent handler.
pub type WorkspaceResolver = Arc<dyn Fn(u32) -> Result<Qiyas, String> + Send + Sync>;
pub async fn serve_workspaces(
    factory: Arc<dyn Fn() -> Qiyas + Send + Sync>,
    port: u16,
    token: String,
    shutdown: CancellationToken,
) -> Result<SocketAddr, AiError> {
    serve_factory(factory, port, token, shutdown).await
}

async fn serve_handler(
    handler: Qiyas,
    port: u16,
    token: String,
    shutdown: CancellationToken,
) -> Result<SocketAddr, AiError> {
    serve_factory(
        Arc::new(move || handler.new_connection()),
        port,
        token,
        shutdown,
    )
    .await
}

async fn serve_factory(
    factory: Arc<dyn Fn() -> Qiyas + Send + Sync>,
    port: u16,
    token: String,
    shutdown: CancellationToken,
) -> Result<SocketAddr, AiError> {
    let listener = bind(port).await?;
    let addr = listener
        .local_addr()
        .map_err(|e| AiError::Server(format!("127.0.0.1:{port}: {e}")))?;

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
        move || Ok(factory()),
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
pub struct Qiyas {
    session: Arc<Mutex<Option<Session>>>,
    host: Option<Arc<dyn DrawingHost>>,
    on_mutated: OnMutated,
    on_ui: OnUi,
    tools: Arc<Vec<Tool>>,
    retries: Arc<Mutex<VecDeque<Retry>>>,
    ineffective: Arc<Mutex<std::collections::BTreeMap<String, usize>>>,
    churn: Arc<Mutex<crate::churn::Guard>>,
    resolver: Option<WorkspaceResolver>,
    addressed: Arc<Mutex<std::collections::BTreeMap<u32, Qiyas>>>,
    source: Source,
    user_text: Option<String>,
}

#[derive(Clone)]
struct Retry {
    id: String,
    input: Value,
    output: Result<ToolOutput, ToolError>,
}

/// MCP accepts a retry id without changing the provider-strict in-app tool schemas.
pub fn tools() -> Vec<crate::ToolSpec> {
    let mut specs = catalogue();
    for spec in &mut specs {
        spec.input_schema["properties"]["workspace"] = serde_json::json!({"type":"integer","minimum":0,
            "description":"Desktop drawing id. Omit to use this connection’s drawing. Switching the visible tab never changes this binding."});
        if spec.name == surface::EXECUTE {
            // MCP accepts omission; strict provider schemas use a required nullable field.
            spec.input_schema["required"]
                .as_array_mut()
                .expect("tool required fields")
                .retain(|field| field != "expected_revision");
            spec.input_schema["properties"]["request_id"] = serde_json::json!({
                "type": "string", "minLength": 1,
                "description": "Optional retry id: identical arguments return the original result, never run twice; retained for this MCP session's last 256 ids."
            });
        }
    }
    specs
}

impl Qiyas {
    pub fn new(
        session: Arc<Mutex<Option<Session>>>,
        host: Option<Arc<dyn DrawingHost>>,
        on_mutated: OnMutated,
        on_ui: OnUi,
    ) -> Qiyas {
        let specs = tools();
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
            retries: Arc::new(Mutex::new(VecDeque::new())),
            ineffective: Arc::default(),
            churn: Arc::default(),
            resolver: None,
            addressed: Arc::default(),
            source: Source::Mcp,
            user_text: None,
        }
    }

    pub fn with_workspaces(mut self, resolver: WorkspaceResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    fn addressed(&self, input: &mut Value) -> Result<Self, ToolError> {
        let Some(id) = input.get("workspace") else {
            return Ok(self.clone());
        };
        let id = id
            .as_u64()
            .and_then(|id| u32::try_from(id).ok())
            .ok_or_else(|| ToolError::Failed("Invalid drawing id".into()))?;
        let resolver = self.resolver.as_ref().ok_or_else(|| {
            ToolError::Failed("This server has one drawing; omit workspace".into())
        })?;
        input
            .as_object_mut()
            .expect("workspace has an object")
            .remove("workspace");
        // Resolve on every request so closing a tab never leaves a writable orphan behind.
        let fresh = resolver(id).map_err(ToolError::Failed)?;
        let mut targets = self
            .addressed
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Ok(targets.entry(id).or_insert(fresh).clone())
    }

    fn new_connection(&self) -> Self {
        Self {
            retries: Arc::new(Mutex::new(VecDeque::new())),
            ineffective: Arc::default(),
            churn: Arc::default(),
            addressed: Arc::default(),
            ..self.clone()
        }
    }

    /// The cache lock also serializes simultaneous retries of one id.
    fn run_cached(&self, name: &str, input: &Value) -> (Result<ToolOutput, ToolError>, bool) {
        let Some(id) = input.get("request_id").filter(|_| name == surface::EXECUTE) else {
            return (self.run(name, input), false);
        };
        let Some(id) = id.as_str().filter(|id| !id.is_empty()) else {
            return (
                Err(ToolError::Structured {
                    code: "invalid_value".into(),
                    message: "request_id must be a nonempty string".into(),
                    details: serde_json::json!({"revision": self.session.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(qiyas_core::Session::revision)}),
                }),
                false,
            );
        };
        let mut arguments = input.clone();
        arguments
            .as_object_mut()
            .expect("request_id has an object")
            .remove("request_id");
        // String and object args are the same request once decoded.
        if let Ok(args) = surface::args_of(&arguments) {
            arguments["args"] = args;
        }
        let mut cache = self.retries.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(kept) = cache.iter().find(|kept| kept.id == id) {
            if kept.input == arguments {
                return (kept.output.clone(), true);
            }
            return (
                Err(ToolError::Structured {
                    code: "request_id_reused".into(),
                    message: "request_id was already used with different arguments".into(),
                    details: serde_json::json!({"revision": self.session.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(qiyas_core::Session::revision)}),
                }),
                true,
            );
        }
        let output =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run(name, &arguments)))
                .unwrap_or_else(|_| {
                    crate::revision::stamp(
                        Err(ToolError::Failed(format!("The tool {name} crashed"))),
                        serde_json::json!(self
                            .session
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .as_ref()
                            .map(Session::revision)),
                    )
                });
        if cache.len() == 256 {
            cache.pop_front();
        }
        cache.push_back(Retry {
            id: id.into(),
            input: arguments,
            output: output.clone(),
        });
        (output, false)
    }

    /// Runs one tool on the session, holding the lock only while it runs; a file tool through the
    /// host, without the session's lock (the host locks the app's tabs first).
    fn run(&self, name: &str, input: &Value) -> Result<ToolOutput, ToolError> {
        let Some(key) = crate::agent::edit_fingerprint(name, input) else {
            return self.run_checked(name, input);
        };
        let revision = self
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(qiyas_core::Session::revision);
        let key = format!("{revision:?}:{key}");
        let mut attempts = self
            .ineffective
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if attempts.get(&key).copied().unwrap_or(0) >= 5 {
            return crate::revision::stamp(
                Err(crate::advice::stopped()),
                serde_json::json!(revision),
            );
        }
        let mut output = self.run_checked(name, input);
        if output
            .as_ref()
            .err()
            .is_some_and(|e| e.json().get("revision").is_none())
        {
            let guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            output = crate::revision::stamp(
                output,
                serde_json::json!(guard.as_ref().map(qiyas_core::Session::revision)),
            );
        }
        if output.is_err() || matches!(&output, Ok(ToolOutput::Json(v)) if v["no_change"] == true) {
            let count = attempts.entry(key).or_default();
            *count += 1;
            if *count >= 2 {
                output = crate::advice::result(output, crate::advice::repeated(*count));
            }
        }
        if input["op"] == "finish" && output.is_ok() {
            attempts.clear();
            self.churn
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .reset();
        }
        output
    }

    fn run_checked(&self, name: &str, input: &Value) -> Result<ToolOutput, ToolError> {
        let (normalized, warnings) = crate::normalise::envelope(name, input)?;
        let input = &normalized;
        self.run_normalized(name, input)
            .map(|output| crate::normalise::output(output, &warnings))
    }

    fn run_normalized(&self, name: &str, input: &Value) -> Result<ToolOutput, ToolError> {
        let route = surface::route(name, input).map_err(ToolError::BadInput)?;
        if let Some(host) = self.host.as_deref() {
            host.before_tool(name, input).map_err(ToolError::Failed)?;
        }
        let op = input[if name == surface::CAPTURE {
            "source"
        } else {
            "op"
        }]
        .as_str()
        .unwrap_or_default();
        let drawing = self
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some();
        surface::check_need(
            name,
            op,
            drawing,
            self.host.as_deref().is_some_and(DrawingHost::has_window),
            self.host.is_some(),
        )?;
        if self.source == Source::Mcp && surface::needs_sheet_authority(&route) {
            if let Some(host) = self.host.as_deref() {
                return host.with_sheet_authority(&mut || self.run_route(route.clone()));
            }
        }
        self.run_route(route)
    }

    fn run_route(&self, route: Route) -> Result<ToolOutput, ToolError> {
        match route {
            Route::Window { tool, input } => {
                let host = self
                    .host
                    .as_deref()
                    .ok_or(ToolError::Failed(crate::host::NO_WINDOW.into()))?;
                {
                    let guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
                    crate::revision::check(guard.as_ref(), &input)?;
                }
                let result =
                    crate::run_window(host, &self.session, tool, &input, crate::prompt::Lang::En)
                        .map_err(ToolError::Failed);
                let guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
                crate::revision::stamp(
                    result,
                    serde_json::json!(guard.as_ref().map(qiyas_core::Session::revision)),
                )
            }
            Route::Host { tool, input } => {
                let host = self
                    .host
                    .as_deref()
                    .ok_or(ToolError::Failed(NO_HOST.into()))?;
                {
                    let guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
                    crate::revision::check(guard.as_ref(), &input)?;
                }
                let result = run_host(host, tool, &input, self.user_text.as_deref())
                    .map(ToolOutput::Json)
                    .map_err(ToolError::Failed);
                let guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
                crate::revision::stamp(
                    result,
                    serde_json::json!(guard.as_ref().map(qiyas_core::Session::revision)),
                )
            }
            Route::Session { tool, op, args } => {
                let mut guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
                if guard.is_none() {
                    if let Some(output) = surface::run_without_drawing(&tool, &op, &args, None) {
                        return output;
                    }
                }
                let session = guard
                    .as_mut()
                    .ok_or_else(|| ToolError::Failed(NO_DRAWING.into()))?;
                let mut churn = self.churn.lock().unwrap_or_else(PoisonError::into_inner);
                let result = churn.run(
                    session,
                    &tool,
                    &op,
                    &args,
                    self.source,
                    &crate::MeasurementMethod::default(),
                );
                if result.is_ok() && tool == surface::EXECUTE && op == "plan" {
                    let plan = surface::read_plan(&args)?;
                    churn.set_phase(
                        plan.phases
                            .iter()
                            .find(|p| p.status == surface::PlanStatus::Active)
                            .map_or("", |p| p.id.as_str()),
                    );
                }
                result
            }
        }
    }
}

impl ServerHandler for Qiyas {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("qiyas", env!("CARGO_PKG_VERSION")))
            .with_instructions(format!(
                "{}\n\nCommands by category:\n{}",
                "Follow the user's request and latest message language. Arabic is simple Modern Standard Arabic unless that message uses a dialect. For a new design on a populated drawing, consider the destination and ask when unclear. Use small phases, one batch per phase, capture and check, and edit by handle; rebuilding returns cost advice. For quantity take-off only, read workflow with language en or ar, then digest and takeoff_kit. Drawing work needs no BOQ or finish call. Read panel_state for questions about the window. Read back changes; after repeated ineffective edits, weigh the advice; an identical input that fails five times is stopped on its sixth attempt. Report open findings when finishing; Qiyas attaches them to the turn record. Advice is information and Mizan decides. Quote the revision of the read you act on as expected_revision. On stale_read, read the changed handles again; never retry blindly. Save only when asked; finish does not save.",
                crate::prompt::command_index()
            ))
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
        let mut input = request.arguments.map_or(Value::Null, Value::Object);
        let target = match self.addressed(&mut input) {
            Ok(target) => target,
            Err(error) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    error.json().to_string(),
                )])
                .into())
            }
        };
        let mutating = surface::mutating(&name, &input);
        // Tools are synchronous and a picture takes a while to draw: off the async workers.
        let this = target.clone();
        let tool = name.clone();
        let (output, replayed) =
            match tokio::task::spawn_blocking(move || this.run_cached(&tool, &input)).await {
                Ok(output) => output,
                Err(error) => (
                    Err(ToolError::Failed(format!(
                        "The tool {name} crashed: {error}"
                    ))),
                    false,
                ),
            };
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
                let mut value = e.json();
                if value.get("revision").is_none() {
                    value["revision"] = serde_json::json!(target
                        .session
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .as_ref()
                        .map(Session::revision));
                }
                return Ok(
                    CallToolResult::error(vec![ContentBlock::text(value.to_string())]).into(),
                );
            }
        };
        if !replayed {
            if mutating {
                (target.on_mutated)();
            }
            if let Some(ui) = ui {
                (target.on_ui)(ui);
            }
        }
        Ok(result.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_routing_keeps_sessions_and_retry_ids_independent() {
        use serde_json::json;
        let a = test_server();
        let b = test_server();
        let open = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let resolver = {
            let a = a.clone();
            let b = b.clone();
            let open = open.clone();
            Arc::new(move |id: u32| {
                if !open.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err("Drawing is closed".into());
                }
                match id {
                    1 => Ok(a.clone()),
                    2 => Ok(b.clone()),
                    _ => Err("No drawing".into()),
                }
            }) as WorkspaceResolver
        };
        let server = a.clone().with_workspaces(resolver);
        for (id, length) in [(1, 3), (2, 7)] {
            let mut input = json!({"workspace":id,"op":"run","request_id":"same-id","args":{"command":{
                "type":"draw","kind":"line","a":[0,0],"b":[length,0]}}});
            let target = server.addressed(&mut input).unwrap();
            assert!(target.run_cached(surface::EXECUTE, &input).0.is_ok());
            assert!(target.run_cached(surface::EXECUTE, &input).1);
        }
        for (target, x, absent) in [(&a, 2.0, 4.0), (&b, 6.0, 8.0)] {
            let session = target.session.lock().unwrap();
            let session = session.as_ref().unwrap();
            assert!(session.pick([x, 0.0], 0.05).is_some());
            assert!(session.pick([absent, 0.0], 0.05).is_none());
            assert_eq!(session.history_steps().0.len(), 1);
        }
        open.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(server
            .addressed(&mut json!({"workspace":2,"op":"state"}))
            .is_err());
        assert!(server.addressed(&mut json!({"workspace":-1})).is_err());
        assert!(a.addressed(&mut json!({"workspace":1})).is_err());
    }

    #[test]
    fn revisions_refuse_stale_edits_and_request_ids_still_deduplicate() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(Mutex::new(Some(qiyas_core::Session::new_blank(
            &dir.path().join("read.dxf"),
            6,
        ))));
        let server = Qiyas::new(session.clone(), None, Arc::new(|| {}), Arc::new(|_| {}));
        let json = |result: Result<ToolOutput, ToolError>| match result.unwrap() {
            ToolOutput::Json(v) => v,
            _ => panic!("json"),
        };
        let read = json(server.run(surface::READ, &serde_json::json!({"op":"state"})));
        let create = serde_json::json!({"op":"run","request_id":"one","expected_revision":read["revision"],"args":{"command":{"type":"draw","kind":"line","a":[0,0],"b":[5,0]}}});
        let (first, cached) = server.run_cached(surface::EXECUTE, &create);
        assert!(!cached);
        let first = json(first);
        assert_ne!(first["revision"], read["revision"]);
        let (again, cached) = server.run_cached(surface::EXECUTE, &create);
        assert!(cached);
        assert_eq!(json(again), first);
        let handle = first["added"][0].clone();
        let mut edit = serde_json::json!({"op":"run","expected_revision":read["revision"],"args":{"command":{"type":"set_property","target":{"handle":handle},"prop":"color","value":"red"}}});
        let before = session.lock().unwrap().as_ref().unwrap().revision();
        let error = server.run(surface::EXECUTE, &edit).unwrap_err().json();
        assert_eq!(error["code"], "stale_read");
        assert_eq!(error["history_complete"], true);
        assert!(error["added"].as_array().unwrap().contains(&handle));
        assert_eq!(session.lock().unwrap().as_ref().unwrap().revision(), before);
        let read = json(server.run(
            surface::READ,
            &serde_json::json!({"op":"records","args":{"handles":[handle],"detail":"full"}}),
        ));
        edit["expected_revision"] = read["revision"].clone();
        let applied = json(server.run(surface::EXECUTE, &edit));
        assert!(applied["changed"].as_array().unwrap().contains(&handle));
        let changed = serde_json::json!({"op":"run","request_id":"one","args":{"command":{"type":"draw","kind":"line","a":[0,0],"b":[6,0]}}});
        let conflict = server
            .run_cached(surface::EXECUTE, &changed)
            .0
            .unwrap_err()
            .json();
        assert_eq!(conflict["code"], "request_id_reused");
        assert!(conflict.get("revision").is_some());
    }

    #[test]
    fn plan_round_trips_without_a_drawing_or_a_mutation() {
        let server = Qiyas::new(
            Arc::new(Mutex::new(None)),
            None,
            Arc::new(|| panic!("a plan must not notify a drawing change")),
            Arc::new(|_| panic!("a plan must not change the view")),
        );
        let phases = serde_json::json!([
            {"id":"sizes","title":"Check sizes","status":"done"},
            {"id":"geometry","title":"Draw geometry","status":"active"}
        ]);
        let input = serde_json::json!({"op":"plan","args":{"phases":phases},"request_id":"plan-1"});
        assert!(!surface::mutating(surface::EXECUTE, &input));
        let (out, cached) = server.run_cached(surface::EXECUTE, &input);
        assert!(!cached);
        let ToolOutput::Json(out) = out.unwrap() else {
            panic!("JSON")
        };
        assert_eq!(
            out,
            serde_json::json!({"plan":{"phases":phases},"revision":null})
        );
        assert!(server.run_cached(surface::EXECUTE, &input).1);
    }

    #[test]
    fn the_server_offers_exactly_the_agents_three_tools() {
        let server = Qiyas::new(
            Arc::new(Mutex::new(None)),
            None,
            Arc::new(|| {}),
            Arc::new(|_| {}),
        );
        let specs = tools();
        assert_eq!(server.tools.len(), 3);
        assert_eq!(server.tools.len(), specs.len());
        for spec in specs {
            if spec.name == surface::EXECUTE {
                assert!(!spec.input_schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("expected_revision")));
            }
            let tool = server.get_tool(spec.name).expect("the same tool");
            assert_eq!(tool.description.as_deref(), Some(spec.description));
            assert_eq!(serde_json::json!(tool.input_schema), spec.input_schema);
        }
        assert!(server.get_tool("measure_area").is_none());
    }

    fn test_server() -> Qiyas {
        Qiyas::new(
            Arc::new(Mutex::new(Some(Session::new_blank(
                std::path::Path::new("retry.dxf"),
                6,
            )))),
            None,
            Arc::new(|| {}),
            Arc::new(|_| {}),
        )
    }

    #[test]
    fn private_claude_mcp_edits_can_be_undone_as_mizans_own_work() {
        let mut server = test_server();
        server.source = Source::Ai;
        let input = serde_json::json!({"op":"run","args":{"command":{
            "type":"draw","kind":"line","a":[0,0],"b":[3,0]
        }}});
        server.run(surface::EXECUTE, &input).unwrap();
        assert_eq!(
            server
                .session
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .history_steps()
                .0
                .len(),
            1
        );
        server
            .run(
                surface::EXECUTE,
                &serde_json::json!({"op":"undo_own","args":{}}),
            )
            .unwrap();
        assert!(server
            .session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .history_steps()
            .0
            .is_empty());
    }

    #[derive(Default)]
    struct TurnHost {
        busy: std::sync::atomic::AtomicBool,
    }

    impl DrawingHost for TurnHost {
        fn with_sheet_authority(
            &self,
            run: &mut dyn FnMut() -> Result<ToolOutput, ToolError>,
        ) -> Result<ToolOutput, ToolError> {
            if self.busy.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(ToolError::Failed("Mizan owns the sheet".into()));
            }
            run()
        }
        fn list(&self) -> Result<Value, String> {
            Ok(serde_json::json!({}))
        }
        fn new_drawing(&self, _units: &str) -> Result<Value, String> {
            panic!("a busy turn must refuse the host change")
        }
        fn open(&self, _path: &str, _user_named: bool) -> Result<Value, String> {
            panic!("a busy turn must refuse the host change")
        }
        fn switch(&self, _id: u32) -> Result<Value, String> {
            panic!("a busy turn must refuse the host change")
        }
        fn save(&self) -> Result<Value, String> {
            Ok(serde_json::json!({}))
        }
        fn close(&self, _id: Option<u32>) -> Result<Value, String> {
            panic!("a busy turn must refuse the host change")
        }
    }

    #[test]
    fn external_sheet_changes_obey_turn_authority_but_reads_and_geometry_still_run() {
        use serde_json::json;
        use std::sync::atomic::Ordering;
        let mut server = test_server();
        let host = Arc::new(TurnHost::default());
        host.busy.store(true, Ordering::SeqCst);
        server.host = Some(host.clone());
        let sheet = json!({"type":"set_sheet", "block":"*Model_Space"});
        let inputs = [
            json!({"op":"run","args":{"command":sheet}}),
            json!({"op":"run","args":{"command":sheet.to_string()}}),
            json!({"op":"run","args":{"command":{"type":"SET_SHEET", "block":"*Model_Space"}}}),
            json!({"op":"run","args":{"command":{"type":"batch","commands":[sheet]}}}),
            json!({"op":"batch","args":{"commands":[{"type":"batch","commands":[sheet]}]}}),
            json!({"op":"type","args":{"line":"SHEET *Model_Space"}}),
            json!({"op":"view","args":{"sheet":"Model"}}),
            json!({"op":"undo_own"}),
            json!({"op":"new_drawing","args":{"units":"m"}}),
            json!({"op":"open_drawing","args":{"path":"drawing.dxf"}}),
            json!({"op":"switch_drawing","args":{"id":1}}),
            json!({"op":"close_drawing"}),
        ];
        for input in inputs {
            let error = server.run(surface::EXECUTE, &input).unwrap_err();
            assert!(
                error.to_string().contains("Mizan owns the sheet"),
                "{input}: {error}"
            );
        }
        server.run(surface::READ, &json!({"op":"state"})).unwrap();
        server
            .run(surface::READ, &json!({"op":"drawings"}))
            .unwrap();
        server
            .run(surface::EXECUTE, &json!({"op":"save_drawing"}))
            .unwrap();
        server
            .run(
                surface::EXECUTE,
                &json!({"op":"run","args":{"command":{
                    "type":"draw","kind":"line","a":[0,0],"b":[1,1]
                }}}),
            )
            .unwrap();
        host.busy.store(false, Ordering::SeqCst);
        server
            .run(
                surface::EXECUTE,
                &json!({"op":"run","args":{"command":sheet}}),
            )
            .unwrap();
    }

    #[test]
    fn retries_replay_results_refuse_changed_arguments_and_do_not_run_twice() {
        let server = test_server();
        let input = serde_json::json!({"op":"run", "request_id":"draw-1", "args":{"command":{
            "type":"draw", "kind":"line", "a":[0,0], "b":[10,0]
        }}});
        let (first, cached) = server.run_cached(surface::EXECUTE, &input);
        assert!(!cached);
        let ToolOutput::Json(first) = first.unwrap() else {
            panic!("JSON")
        };
        let (again, cached) = server.run_cached(surface::EXECUTE, &input);
        assert!(cached);
        let ToolOutput::Json(again) = again.unwrap() else {
            panic!("JSON")
        };
        assert_eq!(first, again);
        assert_eq!(
            server
                .session
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .history_steps()
                .0
                .len(),
            1
        );
        let mut changed = input.clone();
        changed["args"]["command"]["b"] = serde_json::json!([20, 0]);
        let (error, _) = server.run_cached(surface::EXECUTE, &changed);
        assert_eq!(error.unwrap_err().json()["code"], "request_id_reused");
        let independent = server.new_connection();
        assert!(!independent.run_cached(surface::EXECUTE, &input).1);
    }

    #[test]
    fn retry_cache_retains_the_last_256_ids_and_error_results() {
        let server = test_server();
        for index in 0..257 {
            let input = serde_json::json!({"op":"run", "request_id":format!("id-{index}"), "args":{"command":{"type":"not_a_command"}}});
            let (result, cached) = server.run_cached(surface::EXECUTE, &input);
            assert!(result.is_err());
            assert!(!cached);
            assert!(server.run_cached(surface::EXECUTE, &input).1);
        }
        let cache = server.retries.lock().unwrap();
        assert_eq!(cache.len(), 256);
        assert_eq!(cache.front().unwrap().id, "id-1");
        assert_eq!(cache.back().unwrap().id, "id-256");
    }

    #[test]
    fn same_compares_whole_values() {
        assert!(same(b"Bearer abc", b"Bearer abc"));
        assert!(!same(b"Bearer abd", b"Bearer abc"));
        assert!(!same(b"Bearer ab", b"Bearer abc"));
        assert!(!same(b"", b"Bearer abc"));
    }
}
