use super::*;
use crate::{StopReason, Usage};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fake {
    metadata: Option<bool>,
    outcome: Result<bool, AiError>,
    probes: AtomicUsize,
    requests: Mutex<Vec<TurnRequest>>,
}
impl Provider for Fake {
    fn image_input(&self, _: &str) -> Option<bool> {
        self.metadata
    }
    fn probe_image_input<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<bool, AiError>> {
        Box::pin(async move {
            self.probes.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            self.outcome.clone()
        })
    }
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        _: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let mut requests = self.requests.lock().unwrap();
            let images = has_images(&req);
            requests.push(req);
            if images {
                return Err(refusal());
            }
            Ok(TurnResult {
                content: vec![],
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            })
        })
    }
}
fn fake(metadata: Option<bool>, outcome: Result<bool, AiError>) -> Fake {
    Fake {
        metadata,
        outcome,
        probes: AtomicUsize::new(0),
        requests: Mutex::new(vec![]),
    }
}
fn key() -> Key {
    Key {
        provider: "test".into(),
        scope: "account-a".into(),
        model: "model".into(),
    }
}
fn refusal() -> AiError {
    AiError::Api {
        status: 400,
        message: "This model does not support image input".into(),
        body: String::new(),
        request: None,
    }
}
#[tokio::test]
async fn metadata_answers_without_probing_and_survives_reload_with_date() {
    for supported in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pictures.json");
        let cache = Cache::new(path.clone());
        let provider = fake(Some(supported), Ok(!supported));
        assert_eq!(detect(&cache, &key(), &provider).await, Some(supported));
        assert_eq!(provider.probes.load(Ordering::SeqCst), 0);
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value[0]["model"], "model");
        assert!(value[0]["checked_at"].as_str().unwrap().contains('T'));
        assert_eq!(Cache::new(path).get(&key()), Some(supported));
    }
}
#[tokio::test]
async fn unknown_probes_once_and_scopes_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path().join("pictures.json"));
    let provider = fake(None, Ok(true));
    assert_eq!(detect(&cache, &key(), &provider).await, Some(true));
    assert_eq!(detect(&cache, &key(), &provider).await, Some(true));
    assert_eq!(provider.probes.load(Ordering::SeqCst), 1);
    let mut other = key();
    other.scope = "account-b".into();
    assert_eq!(cache.get(&other), None);
    other = key();
    other.model = "another".into();
    assert_eq!(cache.get(&other), None);
}
#[tokio::test]
async fn unrelated_failures_stay_unknown_and_are_not_written() {
    for error in [
        AiError::Auth,
        AiError::RateLimited { retry_after: None },
        AiError::Network("offline".into()),
        AiError::Timeout("timeout".into()),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pictures.json");
        let cache = Cache::new(path.clone());
        assert_eq!(detect(&cache, &key(), &fake(None, Err(error))).await, None);
        assert_eq!(cache.get(&key()), None);
        assert!(!path.exists());
    }
}
#[tokio::test]
async fn explicit_image_refusal_is_cached_as_text_only() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path().join("pictures.json"));
    assert_eq!(
        detect(&cache, &key(), &fake(None, Err(refusal()))).await,
        Some(false)
    );
    assert_eq!(cache.get(&key()), Some(false));
}
#[test]
fn expired_answers_are_not_reused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pictures.json");
    let entry = Entry {
        key: key(),
        image_input: true,
        checked_at: (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339(),
        resolved_model: None,
    };
    std::fs::write(&path, serde_json::to_vec(&vec![entry]).unwrap()).unwrap();
    assert_eq!(Cache::new(path).get(&key()), None);
}
#[tokio::test]
async fn real_image_refusal_retries_without_direct_or_tool_images_and_learns() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(Cache::new(dir.path().join("pictures.json")));
    let provider = Detected::new(Box::new(fake(Some(true), Ok(true))), cache.clone(), key());
    let mut req = probe_request("model");
    req.messages
        .push(Message::user(vec![ContentBlock::ToolResult {
            tool_use_id: "capture".into(),
            content: vec![ResultPart::Png("png".into())],
            is_error: false,
        }]));
    provider.stream_turn(req, &mut |_| {}).await.unwrap();
    assert_eq!(cache.get(&key()), Some(false));
    assert_eq!(provider.image_input("model"), Some(false));
}
#[test]
fn only_explicit_image_capability_errors_match() {
    assert!(image_refusal(&refusal()));
    for (status, message) in [
        (401, "image not supported"),
        (429, "image not supported"),
        (400, "invalid image data"),
        (
            400,
            "reasoning_effort is unsupported; the request contains an image",
        ),
        (400, "Tools are not supported with image input"),
        (400, "API access denied: images not supported"),
        (400, "unsupported image format"),
        (400, "image size exceeds limit"),
        (400, "model not found"),
        (500, "vision not supported"),
    ] {
        assert!(
            !image_refusal(&AiError::Api {
                status,
                message: message.into(),
                body: String::new(),
                request: None
            }),
            "{message}"
        );
    }
}

async fn http_server(replies: Vec<(u16, String)>) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("requires a socket-capable runner");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = vec![];
        for (status, body) in replies {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut bytes = vec![];
            let body_start = loop {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let len = header
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .map(|v| v.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + len {
                        break end + 4;
                    }
                }
            };
            requests.push(serde_json::from_slice(&bytes[body_start..]).unwrap_or(Value::Null));
            socket.write_all(format!("HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
        requests
    });
    (base, task)
}

#[tokio::test]
async fn compatible_metadata_skips_image_requests() {
    for supported in [true, false] {
        let (base, server) = http_server(vec![(
            200,
            serde_json::json!({"data":[{"id":"model","capabilities":{"vision":supported}}]})
                .to_string(),
        )])
        .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().join("pictures.json"));
        let provider = crate::openai::OpenAiCompatible::new(&base, None);
        assert_eq!(detect(&cache, &key(), &provider).await, Some(supported));
        assert_eq!(server.await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn compatible_probe_is_minimal_and_only_definitive_answers_are_saved() {
    for (status, body, expected) in [
        (
            200,
            serde_json::json!({"choices":[{"message":{"content":"OK"},"finish_reason":"length"}]}),
            Some(true),
        ),
        (
            400,
            serde_json::json!({"error":{"message":"This model does not support image input"}}),
            Some(false),
        ),
        (
            401,
            serde_json::json!({"error":{"message":"Invalid API key"}}),
            None,
        ),
        (
            429,
            serde_json::json!({"error":{"code":"rate_limit_exceeded"}}),
            None,
        ),
        (
            400,
            serde_json::json!({"error":{"message":"Invalid image data"}}),
            None,
        ),
    ] {
        let (base, server) = http_server(vec![
            (
                200,
                serde_json::json!({"data":[{"id":"model"}]}).to_string(),
            ),
            (status, body.to_string()),
        ])
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pictures.json");
        let cache = Cache::new(path.clone());
        let provider = crate::openai::OpenAiCompatible::new(&base, None);
        assert_eq!(detect(&cache, &key(), &provider).await, expected);
        assert_eq!(detect(&cache, &key(), &provider).await, expected);
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 2);
        let probe = &requests[1];
        assert_eq!(probe["max_completion_tokens"], 1);
        assert_eq!(probe["messages"][0]["content"][0]["text"], "Describe");
        assert_eq!(
            probe["messages"][0]["content"][1]["image_url"]["url"],
            format!("data:image/png;base64,{PNG}")
        );
        assert!(probe.get("tools").is_none());
        assert_eq!(path.exists(), expected.is_some());
    }
}

#[tokio::test]
async fn compatible_real_request_loses_images_once_and_preserves_tools() {
    let (base, server) = http_server(vec![
        (400, serde_json::json!({"error":{"message":"Image input is not supported by this model"}}).to_string()),
        (200, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".into()),
    ]).await;
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(Cache::new(dir.path().join("pictures.json")));
    let provider = Detected::new(
        Box::new(crate::openai::OpenAiCompatible::new(&base, None)),
        cache.clone(),
        key(),
    );
    let mut request = probe_request("model");
    request.cache_key = "real-turn".into();
    request.tools = crate::catalogue();
    provider.stream_turn(request, &mut |_| {}).await.unwrap();
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].to_string().contains(PNG));
    assert!(!requests[1].to_string().contains(PNG));
    assert!(requests[1].to_string().contains(NOTICE));
    assert_eq!(requests[0]["tools"], requests[1]["tools"]);
    assert_eq!(cache.get(&key()), Some(false));
}

#[tokio::test]
async fn anthropic_probe_uses_an_image_and_one_output_token_without_tools_or_fallback() {
    let (base, server) = http_server(vec![(
        200,
        serde_json::json!({"type":"message","content":[{"type":"text","text":"OK"}]}).to_string(),
    )])
    .await;
    let provider = crate::anthropic::Anthropic::with_url("fake-key".into(), base);
    assert!(provider.probe_image_input("model").await.unwrap());
    let requests = server.await.unwrap();
    assert_eq!(requests[0]["max_tokens"], 1);
    assert_eq!(
        requests[0]["messages"][0]["content"][0]["source"]["data"],
        PNG
    );
    for field in ["tools", "fallbacks", "output_config"] {
        assert!(requests[0].get(field).is_none(), "{field}");
    }
}

#[tokio::test]
async fn another_error_after_the_text_retry_does_not_trigger_a_third_request() {
    let (base, server) = http_server(vec![
        (
            400,
            serde_json::json!({"error":{"message":"Image input is not supported by this model"}})
                .to_string(),
        ),
        (
            401,
            serde_json::json!({"error":{"message":"Invalid API key"}}).to_string(),
        ),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(Cache::new(dir.path().join("pictures.json")));
    let provider = Detected::new(
        Box::new(crate::openai::OpenAiCompatible::new(&base, None)),
        cache.clone(),
        key(),
    );
    assert!(provider
        .stream_turn(probe_request("model"), &mut |_| {})
        .await
        .is_err());
    assert_eq!(server.await.unwrap().len(), 2);
    assert_eq!(cache.get(&key()), Some(false));
}

#[tokio::test]
async fn expired_answers_are_probed_again_and_failed_attempts_can_retry_later() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pictures.json");
    let entry = Entry {
        key: key(),
        image_input: false,
        checked_at: (Utc::now() - chrono::Duration::days(31)).to_rfc3339(),
        resolved_model: None,
    };
    std::fs::write(&path, serde_json::to_vec(&vec![entry]).unwrap()).unwrap();
    let cache = Cache::new(path);
    assert_eq!(
        detect(&cache, &key(), &fake(None, Err(AiError::Auth))).await,
        None
    );
    assert!(!cache.due(&key()));
    cache
        .state
        .lock()
        .unwrap()
        .attempts
        .insert(key(), Instant::now() - Duration::from_secs(301));
    let provider = fake(None, Ok(true));
    assert_eq!(detect(&cache, &key(), &provider).await, Some(true));
    assert_eq!(provider.probes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn simultaneous_checks_share_one_probe() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path().join("pictures.json"));
    let provider = fake(None, Ok(true));
    let key = key();
    let (first, second) = tokio::join!(
        detect(&cache, &key, &provider),
        detect(&cache, &key, &provider)
    );
    assert_eq!(first, Some(true));
    assert_eq!(second, None);
    assert_eq!(cache.get(&key), Some(true));
    assert_eq!(provider.probes.load(Ordering::SeqCst), 1);
}

#[test]
fn modality_metadata_is_explicit_and_never_guessed_from_model_names() {
    for (metadata, expected) in [
        (
            serde_json::json!({"slug":"plan-model","input_modalities":["text","image"]}),
            Some(true),
        ),
        (
            serde_json::json!({"slug":"plan-model","modalities":{"input":["text"]}}),
            Some(false),
        ),
        (
            serde_json::json!({"capabilities":{"vision":{"supported":false}}}),
            Some(false),
        ),
        (
            serde_json::json!({"architecture":{"input_modalities":["text","image"]}}),
            Some(true),
        ),
        (serde_json::json!({"supports_vision":true}), Some(true)),
        (serde_json::json!({"id":"claude-opus-5-5"}), None),
    ] {
        assert_eq!(crate::models::image_input(&metadata), expected);
    }
}

#[tokio::test]
async fn an_owned_tool_loop_can_refuse_an_internal_picture_and_continue_with_text() {
    struct Owned {
        requests: Mutex<Vec<TurnRequest>>,
    }
    impl Provider for Owned {
        fn owns_turn(&self) -> bool {
            true
        }
        fn stream_turn<'a>(
            &'a self,
            request: TurnRequest,
            _: Sink<'a>,
        ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
            Box::pin(async move {
                let mut requests = self.requests.lock().unwrap();
                requests.push(request);
                if requests.len() == 1 {
                    return Err(AiError::Subscription("image_unsupported"));
                }
                assert!(requests[1]
                    .system
                    .contains("Do not repeat completed changes"));
                assert!(requests[1].system.contains(NOTICE));
                Ok(TurnResult {
                    content: vec![],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage::default(),
                })
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(Cache::new(dir.path().join("pictures.json")));
    let provider = Detected::new(
        Box::new(Owned {
            requests: Mutex::new(vec![]),
        }),
        cache.clone(),
        key(),
    );
    let mut request = probe_request("model");
    request.messages = vec![Message::user(vec![ContentBlock::Text {
        text: "Check the drawing".into(),
    }])];
    provider.stream_turn(request, &mut |_| {}).await.unwrap();
    assert_eq!(cache.get(&key()), Some(false));
}
