//! Subscription account commands. Only Qiyas's ChatGPT entry is read from the keychain.
//! Claude credentials and authentication are entirely owned by the installed Claude Code.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use keyring_core::{Entry, Error};
use qiyas_ai::chatgpt_plan::{self, oauth, Account, TokenStore, Vault};
use qiyas_ai::{AiError, Provider};
use serde::Serialize;
use tauri::{AppHandle, Runtime, State};
use tokio_util::sync::CancellationToken;

use crate::state::AppState;

static PENDING: Mutex<Option<CancellationToken>> = Mutex::new(None);

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
pub async fn chatgpt_models() -> Result<Vec<qiyas_ai::models::ModelInfo>, String> {
    plan_provider().models().await.map_err(|e| e.for_window())
}

#[tauri::command]
pub fn cancel_chatgpt_sign_in() {
    if let Some(cancel) = PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        cancel.cancel();
    }
}
fn begin_sign_in() -> CancellationToken {
    cancel_chatgpt_sign_in();
    let cancel = CancellationToken::new();
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = Some(cancel.clone());
    cancel
}

fn stop<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    cancel_chatgpt_sign_in();
    chatgpt_plan::stop_requests();
    if state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .provider
        == crate::settings::Provider::ChatgptPlan
    {
        crate::commands::new_conversation(app, state);
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
    chatgpt_account().await
}

/// Only an explicit user action resumes requests after a plan limit.
#[tauri::command]
pub async fn chatgpt_resume_plan() -> Result<AccountStatus, String> {
    {
        let (operation, mut vault) = chatgpt_plan::load(Arc::new(PlanKeychain))
            .await
            .map_err(|e| e.for_window())?;
        if let Some(r) = vault.active_mut() {
            r.paused = false;
        }
        chatgpt_plan::save(Arc::new(PlanKeychain), vault, operation)
            .await
            .map_err(|e| e.for_window())?;
    }
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
    let mut status = chatgpt_account().await?;
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
    chatgpt_account().await
}

async fn sign_in(
    protocol: oauth::Protocol,
    cancel: CancellationToken,
    store: Arc<dyn TokenStore>,
) -> Result<(), AiError> {
    let (operation, mut vault) = chatgpt_plan::load(store.clone()).await?;
    if cancel.is_cancelled() {
        return Err(AiError::Subscription("chatgpt_cancelled"));
    }
    // An expired authorization code is discarded. One fresh browser authorization is
    // allowed; persistent failure returns to Settings rather than opening a login loop.
    let mut client_id = vault.active().map(|r| r.client_id.clone());
    for attempt in 0..2 {
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
        open_browser(&url).map_err(|_| AiError::Subscription("chatgpt_oauth"))?;
        let grant = oauth::loopback_code(listener, &pending, &cancel).await?;
        client_id = Some(grant.client_id.clone());
        let result = tokio::select! {
            tokens = protocol.exchange(pending, grant) => tokens,
            () = cancel.cancelled() => return Err(AiError::Subscription("chatgpt_cancelled")),
        };
        match result {
            Err(e) if e.code() == "chatgpt_reauthorize" && attempt == 0 => continue,
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

#[tauri::command]
pub fn subscription_link(link: String) -> Result<(), String> {
    let url = match link.as_str() {
        "claude_install" => qiyas_ai::claude_code::INSTALL_URL,
        "chatgpt_limits" => "https://chatgpt.com/#settings",
        _ => return Err("Unknown subscription link".into()),
    };
    open_browser(url)
}

#[cfg(test)]
mod tests {
    use super::*;
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
