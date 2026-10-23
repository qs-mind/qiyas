use super::*;
use crate::{ContentBlock, Message};
use serde_json::json;

// Sign-out and plan pauses act on the whole app (process-wide state), so these tests take turns.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Memory(std::sync::Mutex<Option<String>>);
impl TokenStore for Memory {
    fn load(&self) -> Result<Option<Vault>, AiError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .as_ref()
            .map(|v| serde_json::from_str(v).unwrap()))
    }
    fn save(&self, vault: &Vault) -> Result<(), AiError> {
        *self.0.lock().unwrap() = Some(serde_json::to_string(vault).unwrap());
        Ok(())
    }
}
fn tokens(scopes: &[&str]) -> Tokens {
    Tokens {
        access_token: "fake-access-secret".into(),
        refresh_token: Some("fake-refresh-secret".into()),
        id_token: "fake-id-secret".into(),
        scopes: scopes.iter().map(|s| (*s).into()).collect(),
        expires_at: oauth::now() + 3600,
        client_id: "oaiapp_test".into(),
        subject: "test-person".into(),
        display_name: "Test Surveyor".into(),
    }
}
fn request() -> TurnRequest {
    TurnRequest {
        system: "Mizan".into(),
        model: "mock-model".into(),
        effort: "none".into(),
        max_tokens: 10,
        messages: vec![Message::user(vec![ContentBlock::Text {
            text: "Measure".into(),
        }])],
        tools: crate::catalogue(),
    }
}
fn memory(tokens: Tokens) -> Arc<Memory> {
    let mut vault = Vault::default();
    vault.connect(tokens).unwrap();
    let memory = Arc::new(Memory(std::sync::Mutex::new(None)));
    memory.save(&vault).unwrap();
    memory
}
fn protocol() -> oauth::Protocol {
    oauth::Protocol {
        authorization: "not-a-url".into(),
        token: "not-a-url".into(),
        issuer: "not-a-url".into(),
        discovery: "not-a-url".into(),
        responses: "not-a-url".into(),
        models: "not-a-url".into(),
    }
}
#[tokio::test]
async fn missing_scope_blocks_inference_before_any_network_or_refresh() {
    let _serial = SERIAL.lock().await;
    let mut token = tokens(&["openid", "profile"]);
    token.expires_at = 0;
    let store = memory(token);
    let provider = ChatgptPlan::new(store.clone(), protocol());
    let mut events = Vec::new();
    let error = provider
        .stream_turn(request(), &mut |e| events.push(e))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "chatgpt_scope");
    assert!(events.is_empty());
    assert_eq!(provider.save_state(), json!(null));
    let account = account(store).await.unwrap();
    assert!(account.signed_in);
    assert!(!account.plan_granted);
    let public = serde_json::to_string(&account).unwrap();
    assert!(public.contains("Test Surveyor"));
    for secret in [
        "fake-access-secret",
        "fake-refresh-secret",
        "fake-id-secret",
        "test-person",
        "oaiapp_test",
    ] {
        assert!(!public.contains(secret));
        assert!(!error.for_window().contains(secret));
    }
}
#[test]
fn host_is_opaque_stable_across_sign_out_and_accounts_and_is_keychain_only() {
    let store = memory(tokens(&[PLAN_SCOPE]));
    let mut vault = store.load().unwrap().unwrap();
    let host = vault.host_id.clone();
    assert!(host.starts_with("urn:uuid:"));
    assert_eq!(
        uuid::Uuid::parse_str(&host[9..]).unwrap().get_version_num(),
        4
    );
    assert_ne!(host, Vault::default().host_id);
    assert!(!host.contains('@'));
    vault.disconnect();
    store.save(&vault).unwrap();
    let mut vault = store.load().unwrap().unwrap();
    assert_eq!(vault.host_id, host);
    assert!(vault.active().unwrap().tokens.is_none());
    assert_eq!(vault.active().unwrap().client_id, "oaiapp_test");
    vault.connect(tokens(&[])).unwrap();
    let first = vault.active.clone();
    vault.select(None).unwrap();
    let mut second = tokens(&[PLAN_SCOPE]);
    second.subject = "second-person".into();
    second.client_id = "oaiapp_second".into();
    vault.connect(second).unwrap();
    assert_eq!(vault.registrations.len(), 2);
    assert_eq!(vault.active().unwrap().client_id, "oaiapp_second");
    vault.select(first).unwrap();
    assert_eq!(
        vault.active().unwrap().tokens.as_ref().unwrap().client_id,
        "oaiapp_test"
    );
    assert_eq!(vault.host_id, host);
    let mut wrong = tokens(&[PLAN_SCOPE]);
    wrong.subject = "second-person".into();
    assert_eq!(
        vault.connect(wrong).unwrap_err().code(),
        "chatgpt_account_mismatch"
    );
    assert!(vault.select(Some("unknown".into())).is_err());
    assert!(!serde_json::to_string(&vault.public())
        .unwrap()
        .contains(&host));
    vault.select(None).unwrap();
    let mut another_registration = tokens(&[PLAN_SCOPE]);
    another_registration.client_id = "oaiapp_other_registration".into();
    vault.connect(another_registration).unwrap();
    assert_eq!(
        vault.registrations.len(),
        3,
        "old registrations remain available for revocation"
    );
    vault.select(None).unwrap();
    vault.connect(tokens(&[PLAN_SCOPE])).unwrap();
    assert_eq!(
        vault.registrations.len(),
        3,
        "the same identity and client ID reuse one registration"
    );
}
#[tokio::test]
async fn plan_limits_pause_requests_until_user_resumes_and_never_clear_tokens() {
    let _serial = SERIAL.lock().await;
    let store = memory(tokens(&[PLAN_SCOPE]));
    let provider = ChatgptPlan::new(store.clone(), protocol());
    let id = store.load().unwrap().unwrap().active.unwrap();
    provider
        .record_limit::<()>(&id, &Err(AiError::Subscription("chatgpt_limit")))
        .await
        .unwrap();
    let error = provider
        .stream_turn(request(), &mut |_| panic!("paused request started"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "chatgpt_limit");
    let public = account(store.clone()).await.unwrap();
    assert!(public.signed_in && public.paused);
    let mut vault = store.load().unwrap().unwrap();
    vault.active_mut().unwrap().paused = false;
    store.save(&vault).unwrap();
    assert!(!account(store).await.unwrap().paused);
}
#[test]
fn diagnostics_redact_all_token_material() {
    assert!(!tokens(&[PLAN_SCOPE])
        .scrub("fake-access-secret fake-refresh-secret fake-id-secret")
        .contains("fake-"));
}

/// Synthetic loopback transport. No real service, login or credential is involved.
async fn mock_server(
    replies: Vec<(u16, serde_json::Value)>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("requires a socket-capable runner");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server_base = base.clone();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, reply) in replies {
            let (mut socket, _) =
                tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map(|n| n.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(bytes).unwrap());
            let body = if reply.is_null() {
                String::new()
            } else {
                reply
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| reply.to_string())
                    .replace("MOCK_ORIGIN", &server_base)
            };
            socket.write_all(format!("HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
        requests
    });
    (base, task)
}
fn discovery() -> serde_json::Value {
    json!({"issuer":"https://auth.openai.com","jwks_uri":"MOCK_ORIGIN/jwks","revocation_endpoint":"MOCK_ORIGIN/revoke"})
}
fn local_protocol(base: &str) -> oauth::Protocol {
    oauth::Protocol {
        token: format!("{base}/token"),
        discovery: format!("{base}/discovery"),
        responses: format!("{base}/responses"),
        models: format!("{base}/models"),
        ..oauth::Protocol::official()
    }
}
#[tokio::test]
#[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
async fn mock_refresh_rotates_atomically_and_omits_scope_and_secret() {
    let _serial = SERIAL.lock().await;
    let (base, server) = mock_server(vec![
        (200, discovery()), (200,serde_json::from_str(include_str!("../../tests/fixtures/fake-oidc-jwks.json")).unwrap()),
        (200,json!({"token_type":"Bearer","access_token":"fake-rotated-access","refresh_token":"fake-rotated-refresh","expires_in":3600,"scope":format!("openid {PLAN_SCOPE}")})),
        (200,json!({"models":[{"slug":"mock-slug","display_name":"Mock model","visibility":"list"}]})),
    ]).await;
    let mut old = tokens(&[PLAN_SCOPE]);
    old.expires_at = 0;
    let store = memory(old);
    let provider = ChatgptPlan::new(store.clone(), local_protocol(&base));
    assert_eq!(provider.models().await.unwrap()[0].id, "mock-slug");
    let vault = store.load().unwrap().unwrap();
    let saved = vault.active().unwrap().tokens.as_ref().unwrap();
    assert_eq!(saved.access_token, "fake-rotated-access");
    assert_eq!(saved.refresh_token.as_deref(), Some("fake-rotated-refresh"));
    assert!(saved.expires_at > oauth::now());
    let requests = server.await.unwrap();
    assert!(requests[2].contains("grant_type=refresh_token"));
    assert!(requests[2].contains("client_id=oaiapp_test"));
    assert!(requests[2].contains("resource=https%3A%2F%2Fapi.openai.com%2Fv1"));
    assert!(!requests[2].contains("scope="));
    assert!(!requests[2].contains("client_secret"));
    assert!(requests[3].contains("fake-rotated-access"));
    assert!(!requests[3].contains("fake-access-secret"));
}
#[tokio::test]
#[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
async fn mock_terminal_refresh_clears_only_tokens_and_network_failures_keep_them() {
    let _serial = SERIAL.lock().await;
    for code in [
        "invalid_grant",
        "invalid_refresh_token",
        "token_expired",
        "refresh_token_expired",
        "refresh_token_invalidated",
        "refresh_token_reused",
        "invalid_client",
    ] {
        let (base, server) = mock_server(vec![
            (200, discovery()),
            (
                200,
                serde_json::from_str(include_str!("../../tests/fixtures/fake-oidc-jwks.json"))
                    .unwrap(),
            ),
            (
                400,
                json!({"error":code,"error_description":"fake-refresh-secret"}),
            ),
        ])
        .await;
        let mut old = tokens(&[PLAN_SCOPE]);
        old.expires_at = 0;
        let store = memory(old);
        let host = store.load().unwrap().unwrap().host_id;
        let error = ChatgptPlan::new(store.clone(), local_protocol(&base))
            .models()
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            if code == "invalid_client" {
                "chatgpt_setup"
            } else {
                "chatgpt_reauthorize"
            }
        );
        let vault = store.load().unwrap().unwrap();
        assert_eq!(vault.host_id, host);
        assert_eq!(vault.active().unwrap().client_id, "oaiapp_test");
        assert_eq!(
            vault.active().unwrap().tokens.is_some(),
            code == "invalid_client"
        );
        assert!(!error.for_window().contains("fake-refresh-secret"));
        server.await.unwrap();
    }
    let (base, server) = mock_server(vec![(503, json!({"detail":"fake-refresh-secret"}))]).await;
    let mut old = tokens(&[PLAN_SCOPE]);
    old.expires_at = 0;
    let store = memory(old);
    assert!(ChatgptPlan::new(store.clone(), local_protocol(&base))
        .models()
        .await
        .is_err());
    assert!(store
        .load()
        .unwrap()
        .unwrap()
        .active()
        .unwrap()
        .tokens
        .is_some());
    server.await.unwrap();
}
#[tokio::test]
#[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
async fn mock_revocation_accepts_empty_200_and_retries_server_failure() {
    let _serial = SERIAL.lock().await;
    let (base, server) = mock_server(vec![
        (200, discovery()),
        (503, json!({"detail":"temporary"})),
        (200, discovery()),
        (200, serde_json::Value::Null),
    ])
    .await;
    assert!(local_protocol(&base).revoke(&tokens(&[PLAN_SCOPE])).await);
    let requests = server.await.unwrap();
    for request in [&requests[1], &requests[3]] {
        assert!(request.starts_with("POST /revoke "));
        assert!(request.contains("token=fake-refresh-secret"));
        assert!(request.contains("token_type_hint=refresh_token"));
        assert!(request.contains("client_id=oaiapp_test"));
        assert!(!request.contains("fake-access-secret"));
    }
}

#[tokio::test]
#[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
async fn mock_unsupported_feature_changes_the_next_body_and_reports_activity() {
    let _serial = SERIAL.lock().await;
    let done = json!({"type":"response.completed","response":{"output":[{"type":"message","content":[{"type":"output_text","text":"Measured."}]}],"usage":{}}});
    let (base, server) = mock_server(vec![
        (400,json!({"error":{"code":"subscription_sharing_unsupported_capability","param":"include","message":"fake-id-secret"}})),
        (200,json!(format!("data: {done}\n\n"))),
    ]).await;
    let provider = ChatgptPlan::new(memory(tokens(&[PLAN_SCOPE])), local_protocol(&base));
    let mut events = Vec::new();
    assert!(provider
        .stream_turn(request(), &mut |e| events.push(e))
        .await
        .is_ok());
    let requests = server.await.unwrap();
    let first: serde_json::Value =
        serde_json::from_str(requests[0].split_once("\r\n\r\n").unwrap().1).unwrap();
    let second: serde_json::Value =
        serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_ne!(first, second);
    assert!(first.get("include").is_some());
    assert!(second.get("include").is_none());
    assert_eq!(second["store"], false);
    assert_eq!(second["stream"], true);
    assert!(events
        .iter()
        .any(|e| matches!(e, crate::StreamEvent::Retrying { code, discard_partial: true, .. } if code == "ai.error.chatgpt_capability_removed")));
    assert!(!format!("{events:?}").contains("fake-id-secret"));
}
#[tokio::test]
#[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
async fn mock_admission_limit_stops_next_request_and_401_keeps_credentials() {
    let _serial = SERIAL.lock().await;
    for (status, body, expected) in [
        (
            429,
            json!({"error":{"code":"subscription_sharing_usage_limit_exceeded"}}),
            "chatgpt_limit",
        ),
        (
            401,
            json!({"detail":"fake-access-secret"}),
            "chatgpt_access",
        ),
        (
            403,
            json!({"error":{"code":"subscription_sharing_user_not_eligible"}}),
            "chatgpt_ineligible",
        ),
    ] {
        let (base, server) = mock_server(vec![(status, body)]).await;
        let store = memory(tokens(&[PLAN_SCOPE]));
        let provider = ChatgptPlan::new(store.clone(), local_protocol(&base));
        let mut events = Vec::new();
        let error = provider
            .stream_turn(request(), &mut |e| events.push(e))
            .await
            .unwrap_err();
        assert_eq!(error.code(), expected);
        assert!(!error.for_window().contains("fake-access-secret"));
        assert!(store
            .load()
            .unwrap()
            .unwrap()
            .active()
            .unwrap()
            .tokens
            .is_some());
        assert!(!events
            .iter()
            .any(|e| matches!(e, crate::StreamEvent::Retrying { .. })));
        server.await.unwrap();
        if status == 429 {
            assert_eq!(
                provider
                    .stream_turn(request(), &mut |_| panic!("paused request sent"))
                    .await
                    .unwrap_err()
                    .code(),
                expected
            );
        }
    }
}
#[tokio::test]
#[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
async fn mock_broken_stream_never_succeeds_or_executes_partial_tools() {
    let _serial = SERIAL.lock().await;
    let (base, server) = mock_server(vec![(
        200,
        json!("data: {\"type\":\"response.output_text.delta\",\"delta\":\"Partial\"}\n\n"),
    )])
    .await;
    let provider = ChatgptPlan::new(memory(tokens(&[PLAN_SCOPE])), local_protocol(&base));
    let mut events = Vec::new();
    assert_eq!(
        provider
            .stream_turn(request(), &mut |e| events.push(e))
            .await
            .unwrap_err()
            .code(),
        "chatgpt_incomplete"
    );
    assert!(!events.iter().any(|e| matches!(
        e,
        crate::StreamEvent::Done | crate::StreamEvent::ToolUse { .. }
    )));
    server.await.unwrap();
}

#[tokio::test]
#[ignore = "Requires a socket-capable runner; reviewer runs mock servers serially"]
async fn mock_refresh_is_serial_and_terminal_failure_reauthorizes_once_with_saved_registration() {
    let _serial = SERIAL.lock().await;
    let (base, server) = mock_server(vec![
        (200,discovery()), (200,serde_json::from_str(include_str!("../../tests/fixtures/fake-oidc-jwks.json")).unwrap()),
        (200,json!({"token_type":"Bearer","access_token":"fake-new-access","refresh_token":"fake-new-refresh","expires_in":3600,"scope":PLAN_SCOPE})),
        (200,json!({"models":[]})), (200,json!({"models":[]})),
    ]).await;
    let mut expired = tokens(&[PLAN_SCOPE]);
    expired.expires_at = 0;
    let store = memory(expired);
    let provider = ChatgptPlan::new(store, local_protocol(&base));
    let (first, second) = tokio::join!(provider.models(), provider.models());
    first.unwrap();
    second.unwrap();
    let requests = server.await.unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.starts_with("POST /token "))
            .count(),
        1
    );

    let (base, server) = mock_server(vec![
        (200, discovery()),
        (
            200,
            serde_json::from_str(include_str!("../../tests/fixtures/fake-oidc-jwks.json")).unwrap(),
        ),
        (400, json!({"error":"refresh_token_reused"})),
        (200, json!({"models":[]})),
    ])
    .await;
    let mut expired = tokens(&[PLAN_SCOPE]);
    expired.expires_at = 0;
    let store = memory(expired);
    let saved_store = store.clone();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_inside = calls.clone();
    let provider =
        ChatgptPlan::new(store, local_protocol(&base)).with_reauthorization(Arc::new(move || {
            let store = saved_store.clone();
            let calls = calls_inside.clone();
            Box::pin(async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let (operation, mut vault) = load(store.clone()).await?;
                assert_eq!(vault.active().unwrap().client_id, "oaiapp_test");
                assert!(vault.active().unwrap().tokens.is_none());
                vault.connect(tokens(&[PLAN_SCOPE]))?;
                save(store, vault, operation).await
            })
        }));
    provider.models().await.unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    server.await.unwrap();
}

#[tokio::test]
#[ignore = "Requires a socket-capable runner; reviewer runs mock servers serially"]
async fn mock_sign_out_cancellation_stops_an_inflight_request() {
    let _serial = SERIAL.lock().await;
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let store = memory(tokens(&[PLAN_SCOPE]));
    let provider = ChatgptPlan::new(store.clone(), local_protocol(&base));
    let (sent, received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut chunk = [0; 4096];
        assert!(socket.read(&mut chunk).await.unwrap() > 0);
        sent.send(()).unwrap();
        let mut rest = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            socket.read_to_end(&mut rest),
        )
        .await
        .unwrap()
        .unwrap();
    });
    let request = tokio::spawn(async move { provider.models().await });
    received.await.unwrap();
    stop_requests();
    assert_eq!(
        request.await.unwrap().unwrap_err().code(),
        "chatgpt_cancelled"
    );
    server.await.unwrap();
    // Cancellation itself never erases saved credentials; sign-out revokes before removal.
    assert!(store
        .load()
        .unwrap()
        .unwrap()
        .active()
        .unwrap()
        .tokens
        .is_some());
}

#[tokio::test]
async fn queued_request_cancellation_does_not_wait_or_use_another_account() {
    let _serial = SERIAL.lock().await;
    let operation = TOKEN_OPERATION.lock().await;
    let provider = ChatgptPlan::new(memory(tokens(&[PLAN_SCOPE])), protocol());
    let cancel = tokio_util::sync::CancellationToken::new();
    let request_cancel = cancel.clone();
    let pending = tokio::spawn(async move { provider.tokens_in_session(request_cancel).await });
    tokio::task::yield_now().await;
    cancel.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), pending)
        .await
        .expect("cancellation must not wait for account changes")
        .unwrap();
    assert_eq!(result.err().unwrap().code(), "chatgpt_cancelled");
    drop(operation);
}

struct PausedWrite {
    memory: Arc<Memory>,
    started: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}
impl TokenStore for PausedWrite {
    fn load(&self) -> Result<Option<Vault>, AiError> {
        self.memory.load()
    }
    fn save(&self, vault: &Vault) -> Result<(), AiError> {
        if let Some(started) = self.started.lock().unwrap().take() {
            let _ = started.send(());
            self.release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
        self.memory.save(vault)
    }
}
fn paused_write(
    memory: Arc<Memory>,
) -> (
    Arc<PausedWrite>,
    tokio::sync::oneshot::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    let (started, received) = tokio::sync::oneshot::channel();
    let (release, receiver) = std::sync::mpsc::channel();
    (
        Arc::new(PausedWrite {
            memory,
            started: std::sync::Mutex::new(Some(started)),
            release: std::sync::Mutex::new(receiver),
        }),
        received,
        release,
    )
}
#[tokio::test]
async fn cancelled_keychain_waiter_keeps_the_lock_until_the_write_finishes() {
    let _serial = SERIAL.lock().await;
    let memory = memory(tokens(&[PLAN_SCOPE]));
    let (store, started, release) = paused_write(memory.clone());
    let (operation, mut vault) = load(store.clone()).await.unwrap();
    let host = vault.host_id.clone();
    vault
        .active_mut()
        .unwrap()
        .tokens
        .as_mut()
        .unwrap()
        .refresh_token = Some("fake-replacement".into());
    let saving = tokio::spawn(save(store, vault, operation));
    started.await.unwrap();
    saving.abort();
    assert!(saving.await.unwrap_err().is_cancelled());
    assert!(
        TOKEN_OPERATION.try_lock().is_err(),
        "a cancelled waiter must not release an unfinished write"
    );
    release.send(()).unwrap();
    let (_operation, saved) = load(memory).await.unwrap();
    assert_eq!(saved.host_id, host);
    assert_eq!(
        saved
            .active()
            .unwrap()
            .tokens
            .as_ref()
            .unwrap()
            .refresh_token
            .as_deref(),
        Some("fake-replacement")
    );
}
#[tokio::test]
#[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
async fn mock_cancelled_rotation_commits_before_another_account_operation() {
    let _serial = SERIAL.lock().await;
    let (base, server) = mock_server(vec![
        (200, discovery()), (200, serde_json::from_str(include_str!("../../tests/fixtures/fake-oidc-jwks.json")).unwrap()),
        (200, json!({"token_type":"Bearer","access_token":"fake-new-access","refresh_token":"fake-new-refresh","expires_in":3600,"scope":PLAN_SCOPE})),
    ]).await;
    let mut expired = tokens(&[PLAN_SCOPE]);
    expired.expires_at = 0;
    let memory = memory(expired);
    let (store, started, release) = paused_write(memory.clone());
    let provider = ChatgptPlan::new(store, local_protocol(&base));
    let cancel = tokio_util::sync::CancellationToken::new();
    let request_cancel = cancel.clone();
    let request = tokio::spawn(async move { provider.tokens_in_session(request_cancel).await });
    started.await.unwrap();
    cancel.cancel();
    assert_eq!(
        request.await.unwrap().err().unwrap().code(),
        "chatgpt_cancelled"
    );
    assert!(TOKEN_OPERATION.try_lock().is_err());
    release.send(()).unwrap();
    let (_operation, saved) = load(memory).await.unwrap();
    assert_eq!(
        saved
            .active()
            .unwrap()
            .tokens
            .as_ref()
            .unwrap()
            .refresh_token
            .as_deref(),
        Some("fake-new-refresh")
    );
    assert_eq!(
        server.await.unwrap().len(),
        3,
        "cancellation never starts inference"
    );
}
