//! Official Sign in with ChatGPT. OAuth secrets have no Debug implementation and may only
//! be persisted through TokenStore, implemented by the desktop's OS keychain.
pub mod oauth;
mod responses;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::provider::{BoxFuture, Sink};
use crate::{AiError, Provider, TurnRequest, TurnResult};

pub const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
pub const KEYCHAIN_ACCOUNT: &str = "chatgpt-plan";

/// All token material and account identity stay together in one OS keychain entry.
#[derive(Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: String,
    pub scopes: Vec<String>,
    pub expires_at: u64,
    pub client_id: String,
    pub subject: String,
    pub display_name: String,
}

impl Tokens {
    pub fn require_plan(&self) -> Result<(), AiError> {
        if self.scopes.iter().any(|s| s == PLAN_SCOPE) {
            Ok(())
        } else {
            Err(AiError::Subscription("chatgpt_scope"))
        }
    }

    pub(crate) fn scrub(&self, text: &str) -> String {
        let mut text = crate::provider::scrub(text, &self.access_token);
        text = crate::provider::scrub(&text, &self.id_token);
        if let Some(refresh) = &self.refresh_token {
            text = crate::provider::scrub(&text, refresh);
        }
        text
    }
}

/// Implementations must not write tokens to files, logs or frontend state.
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<Tokens>, AiError>;
    fn save(&self, tokens: &Tokens) -> Result<(), AiError>;
    fn clear(&self) -> Result<(), AiError>;
}

/// Serializes sign-in, refresh and sign-out across all open conversations.
pub static TOKEN_OPERATION: Mutex<()> = Mutex::const_new(());

#[derive(Serialize)]
pub struct Account {
    pub signed_in: bool,
    pub display_name: Option<String>,
    pub plan_granted: bool,
}

pub async fn account(store: Arc<dyn TokenStore>) -> Result<Account, AiError> {
    let _operation = TOKEN_OPERATION.lock().await;
    let tokens = tokio::task::spawn_blocking(move || store.load())
        .await
        .map_err(|_| AiError::Subscription("chatgpt_keychain"))??;
    Ok(Account {
        signed_in: tokens.is_some(),
        plan_granted: tokens.as_ref().is_some_and(|t| t.require_plan().is_ok()),
        display_name: tokens.map(|t| t.display_name),
    })
}

pub struct ChatgptPlan {
    store: Arc<dyn TokenStore>,
    protocol: oauth::Protocol,
}

impl ChatgptPlan {
    pub fn new(store: Arc<dyn TokenStore>, protocol: oauth::Protocol) -> Self {
        Self { store, protocol }
    }
}

impl Provider for ChatgptPlan {
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            // Read the keychain each time, so sign-out also revokes existing conversations.
            let operation = TOKEN_OPERATION.lock().await;
            let store = self.store.clone();
            let mut tokens = tokio::task::spawn_blocking(move || store.load())
                .await
                .map_err(|_| AiError::Subscription("chatgpt_keychain"))??
                .ok_or(AiError::Subscription("chatgpt_auth"))?;
            tokens.require_plan()?;
            if tokens.expires_at <= oauth::now().saturating_add(60) {
                tokens = self.protocol.refresh(tokens).await?;
                let store = self.store.clone();
                tokens = tokio::task::spawn_blocking(move || {
                    store.save(&tokens)?;
                    Ok::<_, AiError>(tokens)
                })
                .await
                .map_err(|_| AiError::Subscription("chatgpt_keychain"))??;
            }
            tokens.require_plan()?;
            drop(operation);
            responses::stream(&self.protocol.responses, &tokens, req, sink).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentBlock, Message};
    use serde_json::json;

    struct Memory(std::sync::Mutex<Option<String>>);
    impl TokenStore for Memory {
        fn load(&self) -> Result<Option<Tokens>, AiError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .as_ref()
                .map(|s| serde_json::from_str(s).unwrap()))
        }
        fn save(&self, tokens: &Tokens) -> Result<(), AiError> {
            *self.0.lock().unwrap() = Some(serde_json::to_string(tokens).unwrap());
            Ok(())
        }
        fn clear(&self) -> Result<(), AiError> {
            *self.0.lock().unwrap() = None;
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
            client_id: "test-client".into(),
            subject: "test-person".into(),
            display_name: "Test Surveyor".into(),
        }
    }
    fn protocol() -> oauth::Protocol {
        oauth::Protocol {
            authorization: "not-a-url".into(),
            token: "not-a-url".into(),
            issuer: "not-a-url".into(),
            jwks: "not-a-url".into(),
            responses: "not-a-url".into(),
            host_parameter: "test-only".into(),
        }
    }
    #[tokio::test]
    async fn missing_scope_blocks_inference_before_any_network_or_refresh() {
        let store = Arc::new(Memory(std::sync::Mutex::new(None)));
        let mut token = tokens(&["openid", "profile"]);
        token.expires_at = 0;
        store.save(&token).unwrap();
        let provider = ChatgptPlan::new(store.clone(), protocol());
        let request = TurnRequest {
            system: "Mizan".into(),
            model: "mock-model".into(),
            effort: "none".into(),
            max_tokens: 10,
            messages: vec![Message::user(vec![ContentBlock::Text {
                text: "Measure".into(),
            }])],
            tools: crate::catalogue(),
        };
        let mut events = Vec::new();
        let error = provider
            .stream_turn(request, &mut |e| events.push(e))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "chatgpt_scope");
        assert!(events.is_empty(), "no inference or retry started");
        assert_eq!(
            provider.save_state(),
            json!(null),
            "conversation has no tokens"
        );
        let account = account(store).await.unwrap();
        let public = serde_json::to_string(&account).unwrap();
        assert!(public.contains("Test Surveyor"));
        for secret in [
            "fake-access-secret",
            "fake-refresh-secret",
            "fake-id-secret",
            "test-person",
        ] {
            assert!(!public.contains(secret));
            assert!(!error.to_string().contains(secret));
            assert!(!error.for_window().contains(secret));
        }
    }
    #[test]
    fn diagnostics_redact_all_token_material() {
        let tokens = tokens(&[PLAN_SCOPE]);
        let diagnostic = tokens.scrub("fake-access-secret fake-refresh-secret fake-id-secret");
        assert!(!diagnostic.contains("fake-"));
    }
}
