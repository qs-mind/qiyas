//! The MCP server end to end: a real rmcp client over HTTP against `serve` on a free port.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use qiyas_ai::mcp::{serve, OnMutated, OnUi};
use qiyas_core::{EntityFilter, Session};
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

type Client = RunningService<RoleClient, ClientConfig>;

struct Server {
    _dir: Option<tempfile::TempDir>,
    session: Arc<Mutex<Option<Session>>>,
    addr: SocketAddr,
    shutdown: CancellationToken,
    mutated: Arc<AtomicUsize>,
    /// The `ui` of every tool result that had one, in order.
    ui: Arc<Mutex<Vec<Value>>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

/// The reqwest client inside rmcp uses rustls with no crypto provider of its own here.
fn crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// `serve` on a free port, with the sample drawing open (or none).
async fn start(with_drawing: bool) -> Server {
    crypto();
    let (dir, session) = if with_drawing {
        let dir = tempfile::tempdir().unwrap();
        let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
        (Some(dir), Some(Session::open(&path).unwrap()))
    } else {
        (None, None)
    };
    let session = Arc::new(Mutex::new(session));
    let shutdown = CancellationToken::new();
    let mutated = Arc::new(AtomicUsize::new(0));
    let counter = mutated.clone();
    let on_mutated: OnMutated = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let ui = Arc::new(Mutex::new(Vec::new()));
    let seen = ui.clone();
    let on_ui: OnUi = Arc::new(move |v| seen.lock().unwrap().push(v));
    let addr = serve(
        session.clone(),
        None,
        0,
        TOKEN.into(),
        shutdown.clone(),
        on_mutated,
        on_ui,
    )
    .await
    .unwrap();
    Server {
        _dir: dir,
        session,
        addr,
        shutdown,
        mutated,
        ui,
    }
}

fn url(addr: SocketAddr) -> String {
    format!("http://{addr}/mcp")
}

async fn connect(addr: SocketAddr, token: &str) -> Result<Client, String> {
    let config = StreamableHttpClientTransportConfig::with_uri(url(addr)).auth_header(token);
    ClientConfig::default()
        .serve(StreamableHttpClientTransport::from_config(config))
        .await
        .map_err(|e| format!("{e:?}"))
}

async fn call(client: &Client, name: &'static str, args: Value) -> CallToolResult {
    let Value::Object(args) = args else {
        panic!("arguments must be an object")
    };
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(args))
        .await
        .unwrap()
}

fn text(result: &CallToolResult, i: usize) -> &str {
    &result.content[i].as_text().expect("a text block").text
}

fn rectangle(server: &Server) -> u64 {
    let guard = server.session.lock().unwrap();
    guard
        .as_ref()
        .unwrap()
        .find_entities(&EntityFilter {
            kind: Some("LwPolyline".into()),
            ..Default::default()
        })
        .rows[0]
        .handle
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lists_the_catalogue_and_measures_as_mcp() {
    let server = start(true).await;
    let client = connect(server.addr, TOKEN).await.unwrap();

    let tools = client.list_all_tools().await.unwrap();
    let specs = qiyas_ai::catalogue();
    assert_eq!(tools.len(), 3);
    for (tool, spec) in tools.iter().zip(specs) {
        assert_eq!(tool.name, spec.name);
        assert_eq!(tool.description.as_deref(), Some(spec.description));
        assert_eq!(
            Value::Object((*tool.input_schema).clone()),
            spec.input_schema
        );
    }

    let handle = rectangle(&server);
    let result = call(
        &client,
        "qiyas_execute",
        json!({"op":"run","args":{"command":{"type":"measure_entity","handle":handle}}}),
    )
    .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let out: Value = serde_json::from_str(text(&result, 0)).unwrap();
    assert_eq!(out["outcome"]["measurement"]["source"], "mcp");
    {
        let guard = server.session.lock().unwrap();
        let items = &guard.as_ref().unwrap().takeoff.items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source, "mcp");
        assert_eq!(items[0].handles, [handle]);
    }
    assert_eq!(server.mutated.load(Ordering::SeqCst), 1);

    // A read-only tool does not count as a change.
    let result = call(&client, "qiyas_read", json!({"op":"state","args":"{}"})).await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert_eq!(server.mutated.load(Ordering::SeqCst), 1);
    client.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sheet_layer_and_zoom_changes_reach_the_window() {
    let server = start(true).await;
    let client = connect(server.addr, TOKEN).await.unwrap();
    for (tool, args) in [
        (
            "qiyas_execute",
            json!({"op":"view","args":{"sheet":"Model"}}),
        ),
        (
            "qiyas_execute",
            json!({"op":"run","args":{"command":{"type":"set_layer_hidden","name":"A-DOOR","hidden":true}}}),
        ),
        (
            "qiyas_execute",
            json!({"op":"view","args":{"bounds":[[0,0],[5,5]]}}),
        ),
        // No ui: nothing is sent.
        ("qiyas_read", json!({"op":"query","args":{"kind":"layers"}})),
    ] {
        let result = call(&client, tool, args).await;
        assert_ne!(result.is_error, Some(true), "{tool}: {result:?}");
    }
    let ui = server.ui.lock().unwrap().clone();
    let actions: Vec<&str> = ui.iter().map(|u| u["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["show_sheet", "redraw", "zoom"]);
    assert_eq!(ui[0]["sheet"], "*Model_Space");
    assert_eq!(ui[1]["hidden"], json!(["A-DOOR"]));
    assert!(server
        .session
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .hidden_layers()
        .iter()
        .any(|name| name == "A-DOOR"));
    // Execute run refreshes the window; view and reads leave it to their ui result.
    assert_eq!(server.mutated.load(Ordering::SeqCst), 1);
    client.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_token_is_refused_with_401() {
    let server = start(true).await;
    let Err(err) = connect(server.addr, "ffffffffffffffffffffffffffffffff").await else {
        panic!("the handshake must fail");
    };
    assert!(err.contains("AuthRequired"), "{err}");

    // No header at all, and a raw request: plain 401.
    let response = reqwest::Client::new()
        .post(url(server.addr))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    assert_eq!(server.mutated.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_counts_both_land() {
    let server = start(true).await;
    let client = connect(server.addr, TOKEN).await.unwrap();
    let (a, b) = tokio::join!(
        call(
            &client,
            "qiyas_execute",
            json!({"op":"run","args":{"command":{"type":"count_block","block_name":"door"}}})
        ),
        call(
            &client,
            "qiyas_execute",
            json!({"op":"run","args":{"command":{"type":"count_block","block_name":"door"}}})
        ),
    );
    for result in [&a, &b] {
        assert_ne!(result.is_error, Some(true), "{result:?}");
        let out: Value = serde_json::from_str(text(result, 0)).unwrap();
        assert_eq!(out["outcome"]["measurement"]["value"], 3.0);
    }
    let guard = server.session.lock().unwrap();
    assert_eq!(guard.as_ref().unwrap().takeoff.items.len(), 2);
    assert_eq!(server.mutated.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn view_image_is_an_image_block() {
    let server = start(true).await;
    let client = connect(server.addr, TOKEN).await.unwrap();
    let result = call(
        &client,
        "qiyas_capture",
        json!({"source":"picture","args":{"width":320}}),
    )
    .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let image = result.content[0].as_image().expect("an image block");
    assert_eq!(image.mime_type, "image/png");
    let png = base64::engine::general_purpose::STANDARD
        .decode(&image.data)
        .unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(text(&result, 1).contains("bounds"));
    assert_eq!(server.mutated.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tool_errors_are_error_results() {
    let server = start(true).await;
    let client = connect(server.addr, TOKEN).await.unwrap();

    let result = call(
        &client,
        "qiyas_execute",
        json!({"op":"run","args":{"command":{"type":"measure_entity","handle":999_999}}}),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(text(&result, 0), "No entity with handle 999999");

    let result = call(&client, "delete_everything", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(server.mutated.load(Ordering::SeqCst), 0);

    // No drawing open.
    let empty = start(false).await;
    let client = connect(empty.addr, TOKEN).await.unwrap();
    let result = call(&client, "qiyas_read", json!({"op":"state","args":"{}"})).await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(text(&result, 0), "No drawing is open");
    // The file tools need the app behind them; without it they say what to do instead.
    let result = call(
        &client,
        "qiyas_execute",
        json!({"op":"new_drawing","args":{"units":"m"}}),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(text(&result, 0), qiyas_ai::host::NO_HOST);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restart_on_the_same_port_binds_again() {
    let server = start(true).await;
    let port = server.addr.port();
    server.shutdown.cancel();
    let shutdown = CancellationToken::new();
    let addr = serve(
        server.session.clone(),
        None,
        port,
        TOKEN.into(),
        shutdown.clone(),
        Arc::new(|| {}),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(addr.port(), port);
    let client = connect(addr, TOKEN).await.unwrap();
    assert_eq!(client.list_all_tools().await.unwrap().len(), 3);
    shutdown.cancel();
}

/// A raw MCP `initialize` POST with the right token and this `Origin`.
async fn post_with_origin(addr: SocketAddr, origin: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .post(url(addr))
        .bearer_auth(TOKEN)
        .header("origin", origin)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#,
        )
        .send()
        .await
        .unwrap()
        .status()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn other_origins_are_refused_even_with_the_token() {
    let server = start(true).await;
    let port = server.addr.port();
    for origin in [
        "http://evil.example",
        "https://evil.example",
        "http://localhost:3000",
        "null",
    ] {
        assert_eq!(
            post_with_origin(server.addr, origin).await,
            reqwest::StatusCode::FORBIDDEN,
            "{origin}"
        );
    }
    // The server's own origin passes.
    let status = post_with_origin(server.addr, &format!("http://127.0.0.1:{port}")).await;
    assert!(status.is_success(), "{status}");

    // Clients that send no Origin (the bridge, Claude Code) still work.
    let client = connect(server.addr, TOKEN).await.unwrap();
    assert_eq!(client.list_all_tools().await.unwrap().len(), 3);
    assert_eq!(server.mutated.load(Ordering::SeqCst), 0);
}
