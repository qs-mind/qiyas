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
#[derive(Clone, Serialize, Deserialize)]
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

/// The entire vault is one atomic OS keychain value. Registrations and the host ID
/// survive sign-out; tokens never move between registrations.
#[derive(Serialize, Deserialize)]
pub struct Vault {
    pub host_id: String,
    pub active: Option<String>,
    pub registrations: Vec<Registration>,
}
#[derive(Serialize, Deserialize)]
pub struct Registration {
    pub id: String,
    pub client_id: String,
    pub subject: String,
    pub display_name: String,
    pub tokens: Option<Tokens>,
    #[serde(default)]
    pub paused: bool,
}
impl Default for Vault {
    fn default() -> Self {
        Self {
            host_id: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            active: None,
            registrations: Vec::new(),
        }
    }
}
impl Vault {
    pub fn active(&self) -> Option<&Registration> {
        self.registrations
            .iter()
            .find(|r| Some(&r.id) == self.active.as_ref())
    }
    pub fn active_mut(&mut self) -> Option<&mut Registration> {
        self.registrations
            .iter_mut()
            .find(|r| Some(&r.id) == self.active.as_ref())
    }
    pub fn connect(&mut self, tokens: Tokens) -> Result<(), AiError> {
        if let Some(r) = self.active_mut() {
            if r.client_id != tokens.client_id || r.subject != tokens.subject {
                return Err(AiError::Subscription("chatgpt_account_mismatch"));
            }
            r.display_name.clone_from(&tokens.display_name);
            r.tokens = Some(tokens);
            r.paused = false;
        } else {
            // The issuer may recognize this host and return an existing registration.
            // Match both identity and client ID; keep other registrations available for sign-out.
            if let Some(r) = self
                .registrations
                .iter_mut()
                .find(|r| r.subject == tokens.subject && r.client_id == tokens.client_id)
            {
                r.display_name.clone_from(&tokens.display_name);
                r.tokens = Some(tokens);
                r.paused = false;
                self.active = Some(r.id.clone());
                return Ok(());
            }
            let id = uuid::Uuid::new_v4().to_string();
            self.registrations.push(Registration {
                id: id.clone(),
                client_id: tokens.client_id.clone(),
                subject: tokens.subject.clone(),
                display_name: tokens.display_name.clone(),
                tokens: Some(tokens),
                paused: false,
            });
            self.active = Some(id);
        }
        Ok(())
    }
    pub fn disconnect(&mut self) {
        if let Some(r) = self.active_mut() {
            r.tokens = None;
            r.paused = false;
        }
    }
    pub fn select(&mut self, id: Option<String>) -> Result<(), AiError> {
        if id
            .as_ref()
            .is_some_and(|id| !self.registrations.iter().any(|r| &r.id == id))
        {
            return Err(AiError::Subscription("chatgpt_account_mismatch"));
        }
        self.active = id;
        Ok(())
    }
    fn public(&self) -> Account {
        let active = self.active();
        let tokens = active.and_then(|r| r.tokens.as_ref());
        Account {
            signed_in: tokens.is_some(),
            plan_granted: tokens.is_some_and(|t| t.require_plan().is_ok()),
            display_name: active.map(|r| r.display_name.clone()),
            active_id: self.active.clone(),
            paused: active.is_some_and(|r| r.paused),
            accounts: self
                .registrations
                .iter()
                .map(|r| AccountChoice {
                    id: r.id.clone(),
                    display_name: r.display_name.clone(),
                    signed_in: r.tokens.is_some(),
                })
                .collect(),
        }
    }
}

/// Implementations must not write the vault to files, logs or frontend state.
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<Vault>, AiError>;
    fn save(&self, vault: &Vault) -> Result<(), AiError>;
}

/// Serializes sign-in, rotation, account switching and sign-out.
pub static TOKEN_OPERATION: Mutex<()> = Mutex::const_new(());
static REQUESTS: std::sync::Mutex<Option<tokio_util::sync::CancellationToken>> =
    std::sync::Mutex::new(None);

pub fn stop_requests() {
    let mut requests = REQUESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(cancel) = requests.take() {
        cancel.cancel();
    }
}
fn request_cancellation() -> tokio_util::sync::CancellationToken {
    REQUESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_or_insert_with(tokio_util::sync::CancellationToken::new)
        .clone()
}

#[derive(Serialize)]
pub struct AccountChoice {
    pub id: String,
    pub display_name: String,
    pub signed_in: bool,
}
#[derive(Serialize)]
pub struct Account {
    pub signed_in: bool,
    pub display_name: Option<String>,
    pub plan_granted: bool,
    pub active_id: Option<String>,
    pub accounts: Vec<AccountChoice>,
    pub paused: bool,
}

/// The blocking worker owns the lock. Cancelling its waiter must not admit another
/// operation while a keychain read, host creation or rotating-token write is still running.
pub async fn load(
    store: Arc<dyn TokenStore>,
) -> Result<(tokio::sync::MutexGuard<'static, ()>, Vault), AiError> {
    let operation = TOKEN_OPERATION.lock().await;
    tokio::task::spawn_blocking(move || {
        let vault = if let Some(vault) = store.load()? {
            vault
        } else {
            let vault = Vault::default();
            store.save(&vault)?;
            vault
        };
        Ok((operation, vault))
    })
    .await
    .map_err(|_| AiError::Subscription("chatgpt_keychain"))?
}
pub async fn save(
    store: Arc<dyn TokenStore>,
    vault: Vault,
    operation: tokio::sync::MutexGuard<'static, ()>,
) -> Result<(), AiError> {
    tokio::task::spawn_blocking(move || {
        let _operation = operation;
        store.save(&vault)
    })
    .await
    .map_err(|_| AiError::Subscription("chatgpt_keychain"))?
}
pub async fn account(store: Arc<dyn TokenStore>) -> Result<Account, AiError> {
    let (_operation, vault) = load(store).await?;
    Ok(vault.public())
}

pub type Reauthorize = Arc<dyn Fn() -> BoxFuture<'static, Result<(), AiError>> + Send + Sync>;
pub struct ChatgptPlan {
    store: Arc<dyn TokenStore>,
    protocol: oauth::Protocol,
    reauthorize: Option<Reauthorize>,
}
impl ChatgptPlan {
    pub fn new(store: Arc<dyn TokenStore>, protocol: oauth::Protocol) -> Self {
        Self {
            store,
            protocol,
            reauthorize: None,
        }
    }
    pub fn with_reauthorization(mut self, callback: Reauthorize) -> Self {
        self.reauthorize = Some(callback);
        self
    }
    async fn tokens(
        &self,
    ) -> Result<(String, Tokens, tokio_util::sync::CancellationToken), AiError> {
        self.tokens_in_session(request_cancellation()).await
    }
    async fn tokens_in_session(
        &self,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<(String, Tokens, tokio_util::sync::CancellationToken), AiError> {
        // Capture cancellation before waiting: queued requests must not move to a newly
        // selected account after sign-out or an account switch.
        // Only terminal refresh failures restart OAuth, once, using the saved registration.
        for attempt in 0..2 {
            let (operation, mut vault) = tokio::select! {
                result = load(self.store.clone()) => result?,
                () = cancel.cancelled() => return Err(AiError::Subscription("chatgpt_cancelled")),
            };
            if cancel.is_cancelled() {
                return Err(AiError::Subscription("chatgpt_cancelled"));
            }
            let registration = vault
                .active_mut()
                .ok_or(AiError::Subscription("chatgpt_auth"))?;
            if registration.paused {
                return Err(AiError::Subscription("chatgpt_limit"));
            }
            let mut tokens = registration
                .tokens
                .take()
                .ok_or(AiError::Subscription("chatgpt_auth"))?;
            tokens.require_plan()?;
            let id = registration.id.clone();
            if tokens.expires_at <= oauth::now().saturating_add(60) {
                let store = self.store.clone();
                let protocol = self.protocol.clone();
                // Once rotation starts, its owner finishes and saves it even if the
                // turn is cancelled. Sign-out then revokes the latest refresh token.
                let rotating = tokio::spawn(async move {
                    match protocol.refresh(tokens).await {
                        Ok(new) => {
                            let registration = vault
                                .active_mut()
                                .ok_or(AiError::Subscription("chatgpt_auth"))?;
                            registration.display_name.clone_from(&new.display_name);
                            registration.tokens = Some(new.clone());
                            save(store, vault, operation).await?;
                            Ok(new)
                        }
                        Err(e) if e.code() == "chatgpt_reauthorize" => {
                            save(store, vault, operation).await?;
                            Err(e)
                        }
                        Err(e) => Err(e), // Saved tokens survive temporary failures.
                    }
                });
                let refreshed = tokio::select! {
                    result = rotating => result.map_err(|_| AiError::Subscription("chatgpt_oauth"))?,
                    () = cancel.cancelled() => return Err(AiError::Subscription("chatgpt_cancelled")),
                };
                match refreshed {
                    Ok(new) => tokens = new,
                    Err(e) if e.code() == "chatgpt_reauthorize" => {
                        if attempt == 0 {
                            if let Some(callback) = &self.reauthorize {
                                tokio::select! {
                                    result = callback() => result?,
                                    () = cancel.cancelled() => return Err(AiError::Subscription("chatgpt_cancelled")),
                                }
                                continue;
                            }
                        }
                        return Err(e);
                    }
                    Err(e) => return Err(e),
                }
            }
            if cancel.is_cancelled() {
                return Err(AiError::Subscription("chatgpt_cancelled"));
            }
            tokens.require_plan()?;
            return Ok((id, tokens, cancel));
        }
        Err(AiError::Subscription("chatgpt_auth"))
    }
    pub async fn models(&self) -> Result<Vec<crate::models::ModelInfo>, AiError> {
        let (id, tokens, cancel) = self.tokens().await?;
        let result = tokio::select! {
            result = responses::models(&self.protocol.models, &tokens) => result,
            () = cancel.cancelled() => Err(AiError::Subscription("chatgpt_cancelled")),
        };
        self.record_limit(&id, &result).await?;
        result
    }
    async fn record_limit<T>(&self, id: &str, result: &Result<T, AiError>) -> Result<(), AiError> {
        if result.as_ref().is_err_and(|e| e.code() == "chatgpt_limit") {
            let (operation, mut vault) = load(self.store.clone()).await?;
            if let Some(r) = vault.registrations.iter_mut().find(|r| r.id == id) {
                r.paused = true;
            }
            save(self.store.clone(), vault, operation).await?;
        }
        Ok(())
    }
}
impl Provider for ChatgptPlan {
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let (id, tokens, cancel) = self.tokens().await?;
            let result = tokio::select! {
                result = responses::stream(&self.protocol.responses, &tokens, req, sink) => result,
                () = cancel.cancelled() => Err(AiError::Subscription("chatgpt_cancelled")),
            };
            self.record_limit(&id, &result).await?;
            result
        })
    }
}

#[cfg(test)]
mod tests;
