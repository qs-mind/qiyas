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
    #[serde(default)]
    pub pending_client_id: Option<String>,
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
    #[serde(default)]
    pub resets_at: Option<u64>,
}
impl Default for Vault {
    fn default() -> Self {
        Self {
            host_id: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            active: None,
            registrations: Vec::new(),
            pending_client_id: None,
        }
    }
}
impl Vault {
    /// The host registration survives incomplete exchange and selecting Add account.
    pub fn sign_in_client_id(&self) -> Option<&str> {
        self.active()
            .map(|r| r.client_id.as_str())
            .or(self.pending_client_id.as_deref())
            .or_else(|| self.registrations.first().map(|r| r.client_id.as_str()))
    }
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
            r.resets_at = None;
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
                r.resets_at = None;
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
                resets_at: None,
            });
            self.active = Some(id);
        }
        Ok(())
    }
    pub fn disconnect(&mut self) {
        if let Some(r) = self.active_mut() {
            r.tokens = None;
            r.paused = false;
            r.resets_at = None;
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
            resets_at: active.and_then(|r| r.resets_at),
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
    pub resets_at: Option<u64>,
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
    let (operation, mut vault) = load(store.clone()).await?;
    let expired = vault
        .active()
        .is_some_and(|r| r.paused && r.resets_at.is_some_and(|at| at <= oauth::now()));
    if expired {
        let r = vault.active_mut().expect("active registration");
        r.paused = false;
        r.resets_at = None;
        let public = vault.public();
        save(store, vault, operation).await?;
        return Ok(public);
    }
    Ok(vault.public())
}

/// A user request may resume immediately after changing the provider's spending settings.
pub async fn resume(store: Arc<dyn TokenStore>) -> Result<(), AiError> {
    let (operation, mut vault) = load(store.clone()).await?;
    if let Some(r) = vault.active_mut() {
        if !r.paused && r.resets_at.is_none() {
            return Ok(());
        }
        r.paused = false;
        r.resets_at = None;
        save(store, vault, operation).await?;
    }
    Ok(())
}

/// Visible account models and only the effort levels explicitly advertised by discovery.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanModel {
    pub id: String,
    pub label: String,
    pub reasoning_efforts: Vec<String>,
    #[serde(default)]
    pub image_input: Option<bool>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
}

pub type Reauthorize = Arc<dyn Fn() -> BoxFuture<'static, Result<(), AiError>> + Send + Sync>;
pub struct ChatgptPlan {
    store: Arc<dyn TokenStore>,
    protocol: oauth::Protocol,
    reauthorize: Option<Reauthorize>,
    models: std::sync::Mutex<Option<(String, Vec<PlanModel>)>>,
    refused_effort: std::sync::Mutex<std::collections::HashSet<String>>,
}
impl ChatgptPlan {
    pub fn new(store: Arc<dyn TokenStore>, protocol: oauth::Protocol) -> Self {
        Self {
            store,
            protocol,
            reauthorize: None,
            models: std::sync::Mutex::new(None),
            refused_effort: std::sync::Mutex::default(),
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
            let (mut operation, mut vault) = tokio::select! {
                result = load(self.store.clone()) => result?,
                () = cancel.cancelled() => return Err(AiError::Subscription("chatgpt_cancelled")),
            };
            if cancel.is_cancelled() {
                return Err(AiError::Subscription("chatgpt_cancelled"));
            }
            if vault
                .active()
                .is_some_and(|r| r.paused && r.resets_at.is_some_and(|at| at <= oauth::now()))
            {
                let r = vault.active_mut().expect("active registration");
                r.paused = false;
                r.resets_at = None;
                save(self.store.clone(), vault, operation).await?;
                (operation, vault) = load(self.store.clone()).await?;
                if cancel.is_cancelled() {
                    return Err(AiError::Subscription("chatgpt_cancelled"));
                }
            }
            let registration = vault
                .active_mut()
                .ok_or(AiError::Subscription("chatgpt_auth"))?;
            if registration.paused {
                return Err(AiError::SubscriptionFailure {
                    code: "chatgpt_limit",
                    resets_at: registration.resets_at,
                    retry_after: None,
                });
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
    pub async fn models(&self) -> Result<Vec<PlanModel>, AiError> {
        let (id, tokens, cancel) = self.tokens().await?;
        let result = tokio::select! {
            result = responses::models(&self.protocol.models, &tokens) => result,
            () = cancel.cancelled() => Err(AiError::Subscription("chatgpt_cancelled")),
        };
        self.record_limit(&id, &result, &cancel).await?;
        if let Ok(models) = &result {
            *self
                .models
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((id, models.clone()));
        }
        result
    }
    async fn configure(
        &self,
        id: &str,
        tokens: &Tokens,
        req: &mut TurnRequest,
    ) -> Result<(), AiError> {
        if req.max_tokens == 0
            || req.model.trim().is_empty()
            || (!req.effort.is_empty() && req.effort != "none")
        {
            let cached = self
                .models
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .filter(|(account, _)| account == id)
                .map(|(_, models)| models.clone());
            let models = if let Some(models) = cached {
                models
            } else {
                let models = responses::models(&self.protocol.models, tokens).await?;
                *self
                    .models
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some((id.to_owned(), models.clone()));
                models
            };
            if req.model.trim().is_empty() {
                req.model = models
                    .first()
                    .ok_or(AiError::Subscription("chatgpt_model"))?
                    .id
                    .clone();
            }
            if req.max_tokens == 0 {
                req.max_tokens = models
                    .iter()
                    .find(|m| m.id == req.model)
                    .and_then(|m| m.max_output_tokens)
                    .unwrap_or(0);
            }
            if !models
                .iter()
                .any(|m| m.id == req.model && m.reasoning_efforts.contains(&req.effort))
            {
                req.effort = "none".into();
            }
        }
        Ok(())
    }
    async fn record_limit<T>(
        &self,
        id: &str,
        result: &Result<T, AiError>,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(), AiError> {
        if result.as_ref().is_err_and(|e| e.code() == "chatgpt_limit") {
            let (operation, mut vault) = load(self.store.clone()).await?;
            // Sign-out or account selection may have completed while this old result
            // waited for the vault. Never restore its pause onto the new session.
            if cancel.is_cancelled() {
                return Ok(());
            }
            if let Some(r) = vault.registrations.iter_mut().find(|r| r.id == id) {
                r.paused = true;
                r.resets_at = result
                    .as_ref()
                    .err()
                    .and_then(|e| e.public_error().failure.resets_at);
            }
            save(self.store.clone(), vault, operation).await?;
        }
        Ok(())
    }
}
impl Provider for ChatgptPlan {
    fn handles_temporary_retries(&self) -> bool {
        true
    }
    fn recovery_output_tokens(&self, _model: &str) -> u32 {
        0
    }
    fn image_input(&self, model: &str) -> Option<bool> {
        self.models.lock().ok().and_then(|models| {
            models
                .as_ref()
                .and_then(|(_, models)| {
                    if model.trim().is_empty() {
                        models.first()
                    } else {
                        models.iter().find(|m| m.id == model)
                    }
                })
                .and_then(|m| m.image_input)
        })
    }
    fn probe_image_input<'a>(&'a self, model: &'a str) -> BoxFuture<'a, Result<bool, AiError>> {
        Box::pin(async move {
            let models = self.models().await?;
            let selected = if model.trim().is_empty() {
                models.first()
            } else {
                models.iter().find(|m| m.id == model)
            };
            if let Some(answer) = selected.and_then(|m| m.image_input) {
                return Ok(answer);
            }
            let mut request =
                crate::pictures::probe_request(selected.map_or(model, |m| m.id.as_str()));
            request.max_tokens = 16;
            self.stream_turn(request, &mut |_| {}).await.map(|_| true)
        })
    }
    fn save_state(&self) -> serde_json::Value {
        let refused = self
            .refused_effort
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if refused.is_empty() {
            serde_json::Value::Null
        } else {
            let mut models: Vec<_> = refused.iter().cloned().collect();
            models.sort();
            serde_json::json!({"refused_effort": models})
        }
    }
    fn restore_state(&self, state: serde_json::Value) {
        *self
            .refused_effort
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state["refused_effort"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
    }

    fn stream_turn<'a>(
        &'a self,
        mut req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let (id, tokens, cancel) = self.tokens().await?;
            let result = tokio::select! {
                result = async {
                    let previous = (req.model.clone(), req.effort.clone());
                    self.configure(&id, &tokens, &mut req).await?;
                    if previous != (req.model.clone(), req.effort.clone()) {
                        sink(crate::StreamEvent::Diagnostic { kind:"CACHE REBUILD".into(), text:"Expected rebuild: model discovery selected the model or supported effort.".into() });
                    }
                    responses::stream(&self.protocol.responses, &tokens, req, &self.refused_effort, sink).await
                } => result,
                () = cancel.cancelled() => Err(AiError::Subscription("chatgpt_cancelled")),
            };
            self.record_limit(&id, &result, &cancel).await?;
            result
        })
    }
}

#[cfg(test)]
mod tests;
