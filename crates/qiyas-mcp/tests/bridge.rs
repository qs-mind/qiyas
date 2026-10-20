//! The bridge binary end to end: an MCP client drives `qiyas-mcp` over stdio, and the bridge
//! talks to the MCP server inside the app (`qiyas_ai::mcp::serve`) over HTTP.

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use qiyas_core::Session;
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceError, ServiceExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const BRIDGE: &str = env!("CARGO_BIN_EXE_qiyas-mcp");

/// The app's MCP server on a free port, with the sample drawing open. Gives its URL.
async fn app(shutdown: &CancellationToken) -> (tempfile::TempDir, String) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Arc::new(Mutex::new(Some(Session::open(&path).unwrap())));
    let addr = qiyas_ai::mcp::serve(
        session,
        None,
        0,
        TOKEN.into(),
        shutdown.clone(),
        Arc::new(|| {}),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    (dir, format!("http://{addr}/mcp"))
}

/// Starts the bridge with these settings and does the MCP handshake with it over stdio.
async fn bridge(url: &str, token: &str) -> RunningService<RoleClient, ClientConfig> {
    let command = Command::new(BRIDGE).configure(|c| {
        c.env("QIYAS_MCP_URL", url).env("QIYAS_MCP_TOKEN", token);
    });
    let (transport, _stderr) = TokioChildProcess::builder(command)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    ClientConfig::default().serve(transport).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lists_and_calls_through_the_bridge() {
    let shutdown = CancellationToken::new();
    let (_dir, url) = app(&shutdown).await;
    let client = bridge(&url, TOKEN).await;

    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(names, ["qiyas_read", "qiyas_execute", "qiyas_capture"]);

    let mut args = rmcp::model::JsonObject::new();
    args.insert("op".into(), "state".into());
    args.insert("args".into(), "{}".into());
    let result = client
        .call_tool(CallToolRequestParams::new("qiyas_read").with_arguments(args))
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let text = &result.content[0].as_text().unwrap().text;
    assert!(text.contains("\"sheets\""), "{text}");

    client.cancel().await.unwrap();
    shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dead_url_answers_not_running() {
    // A port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let client = bridge(&format!("http://127.0.0.1:{port}/mcp"), TOKEN).await;

    let err = client.list_all_tools().await.unwrap_err();
    let ServiceError::McpError(e) = err else {
        panic!("expected an MCP error, got {err:?}");
    };
    assert_eq!(
        e.message,
        "Qiyas is not running or MCP is disabled in its settings"
    );

    let result = client
        .call_tool(CallToolRequestParams::new("list_sheets"))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result.content[0].as_text().unwrap().text,
        "Qiyas is not running or MCP is disabled in its settings"
    );
    client.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_token_says_so() {
    let shutdown = CancellationToken::new();
    let (_dir, url) = app(&shutdown).await;
    let client = bridge(&url, "ffffffffffffffffffffffffffffffff").await;
    let result = client
        .call_tool(CallToolRequestParams::new("list_sheets"))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(result.content[0]
        .as_text()
        .unwrap()
        .text
        .contains("refused the token"));
    client.cancel().await.unwrap();
    shutdown.cancel();
}

/// The bridge as a plain child process, with piped stdin and stdout.
fn spawn() -> tokio::process::Child {
    Command::new(BRIDGE)
        .env("QIYAS_MCP_URL", "http://127.0.0.1:9/mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

async fn exits_cleanly(mut child: tokio::process::Child) {
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("the bridge must exit")
        .unwrap();
    assert!(status.success(), "{status}");
}

#[tokio::test]
async fn exits_when_stdin_closes() {
    // Before the handshake.
    let mut child = spawn();
    drop(child.stdin.take());
    exits_cleanly(child).await;

    // After it.
    let mut child = spawn();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    stdin
        .write_all(
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}
"#,
        )
        .await
        .unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).await.unwrap();
    assert!(line.contains("\"serverInfo\""), "{line}");
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    drop(stdin);
    exits_cleanly(child).await;
}
