//! Subscription account commands. Only Qiyas's ChatGPT entry is read from the keychain.
//! Claude credentials and authentication are entirely owned by the installed Claude Code.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use keyring_core::{Entry, Error};
use qiyas_ai::chatgpt_plan::{self, oauth, Account, PlanModel, TokenStore, Vault};
use qiyas_ai::{AiError, Provider};
use serde::Serialize;
use tauri::{AppHandle, Runtime, State};
use tokio_util::sync::CancellationToken;

use crate::settings::{self, Settings};
use crate::state::AppState;

static PENDING: Mutex<Option<Arc<PendingSignIn>>> = Mutex::new(None);

// Authorization URLs and their state/nonce are backend memory only.
struct PendingSignIn {
    cancel: CancellationToken,
    url: Mutex<Option<String>>,
}
struct SignInGuard(Arc<PendingSignIn>);
impl Drop for SignInGuard {
    fn drop(&mut self) {
        clear_pending(&PENDING, &self.0);
    }
}
fn clear_pending(registry: &Mutex<Option<Arc<PendingSignIn>>>, attempt: &Arc<PendingSignIn>) {
    attempt.cancel.cancel();
    let mut pending = registry.lock().unwrap_or_else(PoisonError::into_inner);
    if pending
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, attempt))
    {
        pending.take();
    }
}
fn replace_pending(registry: &Mutex<Option<Arc<PendingSignIn>>>) -> Arc<PendingSignIn> {
    let mut pending = registry.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(previous) = pending.take() {
        previous.cancel.cancel();
    }
    let attempt = Arc::new(PendingSignIn {
        cancel: CancellationToken::new(),
        url: Mutex::new(None),
    });
    *pending = Some(attempt.clone());
    attempt
}
fn reopen_pending(
    registry: &Mutex<Option<Arc<PendingSignIn>>>,
    open: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    let pending = registry.lock().unwrap_or_else(PoisonError::into_inner);
    let attempt = pending
        .as_ref()
        .filter(|p| !p.cancel.is_cancelled())
        .ok_or_else(|| AiError::Subscription("chatgpt_cancelled").for_window())?;
    let url = attempt.url.lock().unwrap_or_else(PoisonError::into_inner);
    let url = url
        .as_deref()
        .ok_or_else(|| AiError::Subscription("chatgpt_cancelled").for_window())?;
    open(url).map_err(|_| AiError::Subscription("chatgpt_oauth").for_window())
}

pub struct PlanKeychain;
impl PlanKeychain {
    fn entry() -> Result<Entry, AiError> {
        Entry::new("qiyas", chatgpt_plan::KEYCHAIN_ACCOUNT)
            .map_err(|_| AiError::Subscription("chatgpt_keychain"))
    }
}
impl TokenStore for PlanKeychain {
    fn load(&self) -> Result<Option<Vault>, AiError> {
        match Self::entry()?.get_password() {
            Ok(value) => serde_json::from_str(&value)
                .map(Some)
                .map_err(|_| AiError::Subscription("chatgpt_keychain")),
            Err(Error::NoEntry) => Ok(None),
            Err(_) => Err(AiError::Subscription("chatgpt_keychain")),
        }
    }
    fn save(&self, tokens: &Vault) -> Result<(), AiError> {
        let text =
            serde_json::to_string(tokens).map_err(|_| AiError::Subscription("chatgpt_keychain"))?;
        Self::entry()?
            .set_password(&text)
            .map_err(|_| AiError::Subscription("chatgpt_keychain"))
    }
}

fn plan_provider() -> chatgpt_plan::ChatgptPlan {
    chatgpt_plan::ChatgptPlan::new(Arc::new(PlanKeychain), oauth::Protocol::official())
        .with_reauthorization(Arc::new(|| {
            Box::pin(async {
                let cancel = begin_sign_in();
                sign_in(oauth::Protocol::official(), cancel, Arc::new(PlanKeychain)).await
            })
        }))
}
pub fn provider() -> Result<Box<dyn Provider>, String> {
    Ok(Box::new(plan_provider()))
}

#[derive(Serialize)]
pub struct AccountStatus {
    #[serde(flatten)]
    account: Account,
    available: bool,
    notice: Option<&'static str>,
}

#[tauri::command]
pub async fn chatgpt_account() -> Result<AccountStatus, String> {
    let account = chatgpt_plan::account(Arc::new(PlanKeychain))
        .await
        .map_err(|e| e.for_window())?;
    Ok(AccountStatus {
        account,
        available: true,
        notice: None,
    })
}

#[tauri::command]
pub async fn chatgpt_models<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<Vec<PlanModel>, String> {
    let saved = crate::state::lock(&state.settings).clone();
    let observed_identity = if saved.provider == settings::Provider::ChatgptPlan {
        crate::provider_status::identity(&saved).await.ok()
    } else {
        None
    };
    let result = plan_provider().models().await;
    if let Some(identity) = observed_identity.as_ref() {
        crate::provider_status::record_result(
            identity,
            result
                .as_ref()
                .err()
                .map(|error| error.public_error_for("chatgpt_plan")),
        );
    }
    let models = result.map_err(|error| error.for_window())?;
    remember_plan_model(&state.settings, &state.config_dir, &models)?;
    Ok(models)
}

/// Save before updating the cache, so closing Settings cannot lose the automatic choice.
fn remember_plan_model(
    cached: &Mutex<Settings>,
    config_dir: &Path,
    models: &[PlanModel],
) -> Result<bool, String> {
    let mut cached = cached.lock().unwrap_or_else(PoisonError::into_inner);
    let mut next = cached.clone();
    let profile = &mut next.providers.chatgpt_plan;
    if models.iter().any(|model| model.id == profile.model) {
        return Ok(false);
    }
    let selected = models.first();
    profile.model = selected.map_or_else(String::new, |model| model.id.clone());
    if profile.effort.as_ref().is_some_and(|effort| {
        selected.is_none_or(|model| !model.reasoning_efforts.contains(effort))
    }) {
        profile.effort = None;
    }
    if *cached == next {
        return Ok(false);
    }
    settings::save(config_dir, &next)
        .map_err(|e| format!("The settings could not be saved: {e}"))?;
    let changed = crate::ai::answerer_changed(&cached, &next);
    *cached = next;
    Ok(changed)
}

/// Sign-in remains successful when model discovery is temporarily unavailable.
async fn account_with_default_model(state: &AppState) -> Result<AccountStatus, String> {
    remember_plan_model(&state.settings, &state.config_dir, &[])?;
    let mut status = chatgpt_account().await?;
    if status.account.signed_in && status.account.plan_granted && !status.account.paused {
        match plan_provider().models().await {
            Ok(models) => {
                remember_plan_model(&state.settings, &state.config_dir, &models)?;
            }
            Err(error) => {
                // Discovery may have paused the account after a plan limit.
                status = chatgpt_account().await?;
                status.notice = Some(error.code());
            }
        }
    }
    Ok(status)
}

/// Older Settings files may still have no model when the first message is sent.
pub async fn ensure_plan_model(state: &AppState, draft: &mut Settings) -> Result<(), String> {
    if draft.provider != settings::Provider::ChatgptPlan
        || !draft.providers.chatgpt_plan.model.is_empty()
    {
        return Ok(());
    }
    let models = plan_provider().models().await.map_err(|e| e.for_window())?;
    if models.is_empty() {
        return Err(AiError::Subscription("chatgpt_model").for_window());
    }
    remember_plan_model(&state.settings, &state.config_dir, &models)?;
    draft.providers.chatgpt_plan = state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .providers
        .chatgpt_plan
        .clone();
    state
        .provider
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .providers
        .chatgpt_plan = draft.providers.chatgpt_plan.clone();
    Ok(())
}

#[tauri::command]
pub fn cancel_chatgpt_sign_in() {
    if let Some(attempt) = PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        attempt.cancel.cancel();
    }
}
#[tauri::command]
pub fn reopen_chatgpt_sign_in() -> Result<(), String> {
    reopen_pending(&PENDING, open_browser)
}
fn begin_sign_in() -> SignInGuard {
    SignInGuard(replace_pending(&PENDING))
}

fn stop<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    cancel_chatgpt_sign_in();
    chatgpt_plan::stop_requests();
    for workspace in state.conversations() {
        if crate::state::lock(&workspace.provider).provider == settings::Provider::ChatgptPlan {
            if let Ok(bound) = state.scoped(Some(workspace.id)) {
                crate::commands::new_conversation(app, &bound);
            }
        }
    }
}

#[tauri::command]
pub async fn chatgpt_select_account<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: Option<String>,
) -> Result<AccountStatus, String> {
    stop(&app, &state);
    {
        let (operation, mut vault) = chatgpt_plan::load(Arc::new(PlanKeychain))
            .await
            .map_err(|e| e.for_window())?;
        chatgpt_plan::stop_requests();
        vault.select(id).map_err(|e| e.for_window())?;
        chatgpt_plan::save(Arc::new(PlanKeychain), vault, operation)
            .await
            .map_err(|e| e.for_window())?;
    }
    crate::provider_status::clear_provider("chatgpt_plan");
    account_with_default_model(&state).await
}

/// Only an explicit user action resumes requests after a plan limit.
#[tauri::command]
pub async fn chatgpt_resume_plan() -> Result<AccountStatus, String> {
    chatgpt_plan::resume(Arc::new(PlanKeychain))
        .await
        .map_err(|e| e.for_window())?;
    crate::provider_status::clear_provider("chatgpt_plan");
    chatgpt_account().await
}

#[tauri::command]
pub async fn chatgpt_sign_out<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<AccountStatus, String> {
    stop(&app, &state);
    let confirmed;
    {
        let (operation, mut vault) = chatgpt_plan::load(Arc::new(PlanKeychain))
            .await
            .map_err(|e| e.for_window())?;
        chatgpt_plan::stop_requests();
        confirmed = match vault.active().and_then(|r| r.tokens.as_ref()) {
            Some(tokens) => oauth::Protocol::official().revoke(tokens).await,
            None => true,
        };
        vault.disconnect();
        chatgpt_plan::save(Arc::new(PlanKeychain), vault, operation)
            .await
            .map_err(|e| e.for_window())?;
    }
    crate::provider_status::clear_provider("chatgpt_plan");
    let mut status = chatgpt_account().await?;
    remember_plan_model(&state.settings, &state.config_dir, &[])?;
    if !confirmed {
        status.notice = Some("chatgpt_revocation_unconfirmed");
    }
    Ok(status)
}

#[tauri::command]
pub async fn chatgpt_sign_in<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<AccountStatus, String> {
    stop(&app, &state);
    sign_in(
        oauth::Protocol::official(),
        begin_sign_in(),
        Arc::new(PlanKeychain),
    )
    .await
    .map_err(|e| e.for_window())?;
    crate::provider_status::clear_provider("chatgpt_plan");
    account_with_default_model(&state).await
}

async fn sign_in(
    protocol: oauth::Protocol,
    attempt: SignInGuard,
    store: Arc<dyn TokenStore>,
) -> Result<(), AiError> {
    let cancel = &attempt.0.cancel;
    let (mut operation, mut vault) = chatgpt_plan::load(store.clone()).await?;
    if cancel.is_cancelled() {
        return Err(AiError::Subscription("chatgpt_cancelled"));
    }
    // An expired authorization code is discarded. One fresh browser authorization is
    // allowed; persistent failure returns to Settings rather than opening a login loop.
    let mut client_id = vault.sign_in_client_id().map(str::to_owned);
    for retry in 0..2 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| AiError::Subscription("chatgpt_oauth"))?;
        let port = listener
            .local_addr()
            .map_err(|_| AiError::Subscription("chatgpt_oauth"))?
            .port();
        let pending = oauth::Pending::new(
            format!("http://127.0.0.1:{port}/auth/callback"),
            client_id.clone(),
        )?;
        let url = pending.authorization_url(&protocol, &vault.host_id)?;
        {
            let current = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
            if !current.as_ref().is_some_and(|p| Arc::ptr_eq(p, &attempt.0))
                || cancel.is_cancelled()
            {
                return Err(AiError::Subscription("chatgpt_cancelled"));
            }
            *attempt.0.url.lock().unwrap_or_else(PoisonError::into_inner) = Some(url.clone());
            open_browser(&url).map_err(|_| AiError::Subscription("chatgpt_oauth"))?;
        }
        let grant = oauth::loopback_code(listener, &pending, cancel).await?;
        client_id = Some(grant.client_id.clone());
        // The issued ID is only known after a validated callback. Abandoning consent
        // before that point cannot retain an ID that the issuer has not sent to Qiyas.
        (operation, vault) =
            remember_registration(store.clone(), vault, operation, grant.client_id.clone()).await?;
        if cancel.is_cancelled() {
            return Err(AiError::Subscription("chatgpt_cancelled"));
        }
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(AiError::Subscription("chatgpt_cancelled")),
            tokens = protocol.exchange(pending, grant) => tokens,
        };
        match result {
            Err(e) if e.code() == "chatgpt_reauthorize" && retry == 0 => continue,
            Err(e) => return Err(e),
            Ok(tokens) => {
                if cancel.is_cancelled() {
                    return Err(AiError::Subscription("chatgpt_cancelled"));
                }
                vault.connect(tokens)?;
                return chatgpt_plan::save(store, vault, operation).await;
            }
        }
    }
    Err(AiError::Subscription("chatgpt_oauth"))
}

/// Persist the issued client before exchanging the code; retain the operation lock.
async fn remember_registration(
    store: Arc<dyn TokenStore>,
    mut vault: Vault,
    operation: tokio::sync::MutexGuard<'static, ()>,
    client_id: String,
) -> Result<(tokio::sync::MutexGuard<'static, ()>, Vault), AiError> {
    vault.pending_client_id = Some(client_id);
    tokio::task::spawn_blocking(move || {
        store.save(&vault)?;
        Ok((operation, vault))
    })
    .await
    .map_err(|_| AiError::Subscription("chatgpt_keychain"))?
}

/// Only fixed official links or the internally constructed authorization URL reach the OS.
fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    let mut child = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "The browser could not be opened.".to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Fixed identifiers only: never accepts a URL supplied by the UI or a service error.
fn subscription_url(link: &str) -> Result<&'static str, String> {
    match link {
        "claude_install" => Ok(qiyas_ai::claude_code::INSTALL_URL),
        "chatgpt_limits" | "chatgpt_usage" => Ok("https://chatgpt.com/#settings"),
        "claude_usage" => Ok("https://claude.ai/settings/usage"),
        "openai_billing" => {
            Ok("https://platform.openai.com/settings/organization/billing/overview")
        }
        "anthropic_billing" => Ok("https://platform.claude.com/settings/billing"),
        _ => Err("Unknown subscription link".into()),
    }
}

#[tauri::command]
pub fn subscription_link(link: String) -> Result<(), String> {
    open_browser(subscription_url(&link)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_pages_are_fixed_official_urls_and_arbitrary_urls_are_refused() {
        for (key, expected) in [
            ("chatgpt_usage", "https://chatgpt.com/#settings"),
            ("claude_usage", "https://claude.ai/settings/usage"),
            (
                "openai_billing",
                "https://platform.openai.com/settings/organization/billing/overview",
            ),
            (
                "anthropic_billing",
                "https://platform.claude.com/settings/billing",
            ),
        ] {
            assert_eq!(subscription_url(key).unwrap(), expected);
        }
        assert_eq!(
            subscription_url("chatgpt_limits"),
            subscription_url("chatgpt_usage")
        );
        assert!(subscription_url("claude_install").is_ok());
        for invalid in [
            "https://evil.example",
            "javascript:alert(1)",
            "file:///tmp/credentials",
            "openai_billing?redirect=evil",
            "",
        ] {
            assert!(subscription_url(invalid).is_err());
        }
    }

    #[test]
    fn reopening_keeps_the_same_url_and_old_cleanup_cannot_clear_a_new_attempt() {
        let registry = Mutex::new(None);
        let first = replace_pending(&registry);
        *first.url.lock().unwrap() =
            Some("https://auth.openai.com/authorize?state=fake-first".into());
        let mut opened = Vec::new();
        reopen_pending(&registry, |url| {
            opened.push(url.to_owned());
            Ok(())
        })
        .unwrap();
        reopen_pending(&registry, |url| {
            opened.push(url.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(opened[0], opened[1]);
        let second = replace_pending(&registry);
        assert!(first.cancel.is_cancelled());
        clear_pending(&registry, &first);
        assert!(Arc::ptr_eq(
            registry.lock().unwrap().as_ref().unwrap(),
            &second
        ));
        clear_pending(&registry, &second);
        assert!(
            reopen_pending(&registry, |_| panic!("completed attempt must not reopen")).is_err()
        );
    }

    #[test]
    fn registration_selection_survives_add_account_and_legacy_vaults() {
        let mut vault: Vault = serde_json::from_value(serde_json::json!({
            "host_id": "urn:uuid:814178b2-ff06-4415-98ed-41518f0f88a8",
            "active": null,
            "registrations": [{ "id": "saved", "client_id": "oaiapp_saved", "subject": "person",
                "display_name": "Surveyor", "tokens": null }]
        }))
        .unwrap();
        assert_eq!(vault.sign_in_client_id(), Some("oaiapp_saved"));
        vault.pending_client_id = Some("oaiapp_pending".into());
        assert_eq!(vault.sign_in_client_id(), Some("oaiapp_pending"));
        vault.select(Some("saved".into())).unwrap();
        assert_eq!(vault.sign_in_client_id(), Some("oaiapp_saved"));
        vault.select(None).unwrap();
        assert_eq!(vault.sign_in_client_id(), Some("oaiapp_pending"));
    }

    #[tokio::test]
    async fn issued_registration_is_saved_before_an_abandoned_exchange_and_reused() {
        #[derive(Default)]
        struct Store(Mutex<Option<String>>);
        impl TokenStore for Store {
            fn load(&self) -> Result<Option<Vault>, AiError> {
                Ok(self
                    .0
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|s| serde_json::from_str(s).unwrap()))
            }
            fn save(&self, vault: &Vault) -> Result<(), AiError> {
                *self.0.lock().unwrap() = Some(serde_json::to_string(vault).unwrap());
                Ok(())
            }
        }
        let store = Arc::new(Store::default());
        let (operation, vault) = chatgpt_plan::load(store.clone()).await.unwrap();
        let host = vault.host_id.clone();
        let (operation, vault) =
            remember_registration(store.clone(), vault, operation, "oaiapp_issued".into())
                .await
                .unwrap();
        // No exchange follows: cancellation, timeout or a network failure must retain the ID.
        drop(vault);
        drop(operation);
        let (_operation, restored) = chatgpt_plan::load(store).await.unwrap();
        assert_eq!(restored.host_id, host);
        assert_eq!(restored.sign_in_client_id(), Some("oaiapp_issued"));
        assert!(restored.active().is_none());
        let pending = oauth::Pending::new(
            "http://127.0.0.1:43123/auth/callback".into(),
            restored.sign_in_client_id().map(str::to_owned),
        )
        .unwrap();
        let url = pending
            .authorization_url(&oauth::Protocol::official(), &restored.host_id)
            .unwrap();
        let fields: std::collections::HashMap<_, _> = url
            .split_once('?')
            .unwrap()
            .1
            .split('&')
            .filter_map(|field| field.split_once('='))
            .collect();
        assert_eq!(fields["client_id"], "oaiapp_issued");
        assert!(
            !fields.contains_key("agent_name_hint"),
            "retries must not request dynamic registration"
        );
    }

    #[test]
    fn discovered_default_model_is_saved_and_explicit_choices_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let initial = Settings {
            provider: settings::Provider::ChatgptPlan,
            ..Settings::default()
        };
        let cached = Mutex::new(initial);
        let models = vec![
            PlanModel {
                image_input: None,
                id: "first-plan-model".into(),
                label: "First model".into(),
                reasoning_efforts: vec!["low".into(), "high".into()],
            },
            PlanModel {
                image_input: None,
                id: "second-plan-model".into(),
                label: "Second model".into(),
                reasoning_efforts: vec![],
            },
        ];
        assert!(remember_plan_model(&cached, dir.path(), &models).unwrap());
        assert_eq!(
            settings::load(dir.path()).providers.chatgpt_plan.model,
            "first-plan-model"
        );
        assert_eq!(
            cached.lock().unwrap().providers.chatgpt_plan.model,
            "first-plan-model"
        );

        {
            let mut settings = cached.lock().unwrap();
            settings.providers.chatgpt_plan.model = "second-plan-model".into();
            settings::save(dir.path(), &settings).unwrap();
        }
        assert!(!remember_plan_model(&cached, dir.path(), &models).unwrap());
        assert_eq!(
            settings::load(dir.path()).providers.chatgpt_plan.model,
            "second-plan-model"
        );

        assert!(remember_plan_model(&cached, dir.path(), &[]).unwrap());
        assert_eq!(settings::load(dir.path()).providers.chatgpt_plan.model, "");
        assert!(remember_plan_model(&cached, dir.path(), &models[1..]).unwrap());
        assert_eq!(
            settings::load(dir.path()).providers.chatgpt_plan.model,
            "second-plan-model"
        );
    }

    #[test]
    fn model_discovery_does_not_update_memory_when_settings_cannot_be_saved() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("not-a-directory");
        std::fs::write(&blocked, "test").unwrap();
        let cached = Mutex::new(Settings::default());
        let model = PlanModel {
            image_input: None,
            id: "first-plan-model".into(),
            label: "First model".into(),
            reasoning_efforts: vec![],
        };
        assert!(remember_plan_model(&cached, &blocked, &[model]).is_err());
        assert!(cached
            .lock()
            .unwrap()
            .providers
            .chatgpt_plan
            .model
            .is_empty());
    }

    #[test]
    fn vault_lives_only_in_the_dedicated_mock_keychain_and_host_survives_sign_out() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let mut vault = Vault::default();
        let host = vault.host_id.clone();
        vault
            .connect(chatgpt_plan::Tokens {
                access_token: "fake-access".into(),
                refresh_token: Some("fake-refresh".into()),
                id_token: "fake-id".into(),
                scopes: vec![chatgpt_plan::PLAN_SCOPE.into()],
                expires_at: oauth::now() + 3600,
                client_id: "oaiapp_fake".into(),
                subject: "fake-person".into(),
                display_name: "Test Surveyor".into(),
            })
            .unwrap();
        PlanKeychain.save(&vault).unwrap();
        let saved = Entry::new("qiyas", "chatgpt-plan")
            .unwrap()
            .get_password()
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            value["registrations"][0]["tokens"]["access_token"],
            "fake-access"
        );
        let mut saved = PlanKeychain.load().unwrap().unwrap();
        assert_eq!(saved.host_id, host);
        saved.disconnect();
        PlanKeychain.save(&saved).unwrap();
        let signed_out = PlanKeychain.load().unwrap().unwrap();
        assert_eq!(signed_out.host_id, host);
        assert_eq!(signed_out.active().unwrap().client_id, "oaiapp_fake");
        assert!(signed_out.active().unwrap().tokens.is_none());
        assert!(crate::secrets::get_api_key("claude_code").is_err());
        assert!(crate::secrets::get_api_key("chatgpt_plan").is_err());
        let settings = serde_json::to_string(&crate::settings::Settings::default()).unwrap();
        for secret in ["fake-access", "fake-refresh", "fake-id", host.as_str()] {
            assert!(!settings.contains(secret));
        }
    }
}
