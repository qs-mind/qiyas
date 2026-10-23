//! Subscription account commands. Only Qiyas's ChatGPT entry is read from the keychain.
//! Claude credentials and authentication are entirely owned by the installed Claude Code.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use keyring_core::{Entry, Error};
use qiyas_ai::chatgpt_plan::{self, oauth, Account, TokenStore, Tokens, TOKEN_OPERATION};
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
    fn load(&self) -> Result<Option<Tokens>, AiError> {
        match Self::entry()?.get_password() {
            Ok(value) => serde_json::from_str(&value)
                .map(Some)
                .map_err(|_| AiError::Subscription("chatgpt_keychain")),
            Err(Error::NoEntry) => Ok(None),
            Err(_) => Err(AiError::Subscription("chatgpt_keychain")),
        }
    }
    fn save(&self, tokens: &Tokens) -> Result<(), AiError> {
        let text =
            serde_json::to_string(tokens).map_err(|_| AiError::Subscription("chatgpt_keychain"))?;
        Self::entry()?
            .set_password(&text)
            .map_err(|_| AiError::Subscription("chatgpt_keychain"))
    }
    fn clear(&self) -> Result<(), AiError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(_) => Err(AiError::Subscription("chatgpt_keychain")),
        }
    }
}

/// Do not guess registration parameter names or a token audience from policy excerpts.
/// This remains closed until the reviewer supplies the verified SIWC wire specification.
fn reviewed_protocol() -> Result<oauth::Protocol, AiError> {
    Err(AiError::Subscription("chatgpt_setup"))
}

pub fn provider() -> Result<Box<dyn Provider>, String> {
    let protocol = reviewed_protocol().map_err(|e| e.for_window())?;
    Ok(Box::new(chatgpt_plan::ChatgptPlan::new(
        Arc::new(PlanKeychain),
        protocol,
    )))
}

#[derive(Serialize)]
pub struct AccountStatus {
    #[serde(flatten)]
    account: Account,
    available: bool,
}

#[tauri::command]
pub async fn chatgpt_account() -> Result<AccountStatus, String> {
    if reviewed_protocol().is_err() {
        return Ok(AccountStatus {
            account: Account {
                signed_in: false,
                display_name: None,
                plan_granted: false,
            },
            available: false,
        });
    }
    let account = chatgpt_plan::account(Arc::new(PlanKeychain))
        .await
        .map_err(|e| e.for_window())?;
    Ok(AccountStatus {
        account,
        available: reviewed_protocol().is_ok(),
    })
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

#[tauri::command]
pub async fn chatgpt_sign_out<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    cancel_chatgpt_sign_in();
    if state
        .settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .provider
        == crate::settings::Provider::ChatgptPlan
    {
        crate::commands::new_conversation(&app, &state);
    }
    let _operation = TOKEN_OPERATION.lock().await;
    tokio::task::spawn_blocking(|| PlanKeychain.clear())
        .await
        .map_err(|_| AiError::Subscription("chatgpt_keychain").for_window())?
        .map_err(|e| e.for_window())
}

#[tauri::command]
pub async fn chatgpt_sign_in<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<AccountStatus, String> {
    let protocol = reviewed_protocol().map_err(|e| e.for_window())?;
    cancel_chatgpt_sign_in();
    let cancel = CancellationToken::new();
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = Some(cancel.clone());
    let home = qiyas_core::data_home::qiyas_dir()?;
    let result = sign_in(protocol, home, cancel, Arc::new(PlanKeychain)).await;
    if result.is_ok()
        && state
            .settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .provider
            == crate::settings::Provider::ChatgptPlan
    {
        crate::commands::new_conversation(&app, &state);
    }
    result.map_err(|e| e.for_window())?;
    chatgpt_account().await
}

async fn sign_in(
    protocol: oauth::Protocol,
    home: PathBuf,
    cancel: CancellationToken,
    store: Arc<dyn TokenStore>,
) -> Result<(), AiError> {
    let _operation = TOKEN_OPERATION.lock().await;
    let host = tokio::task::spawn_blocking(move || oauth::host_id(&home))
        .await
        .map_err(|_| AiError::Subscription("chatgpt_oauth"))??;
    let saved_store = store.clone();
    let previous = tokio::task::spawn_blocking(move || saved_store.load())
        .await
        .map_err(|_| AiError::Subscription("chatgpt_keychain"))??;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| AiError::Subscription("chatgpt_oauth"))?;
    let port = listener
        .local_addr()
        .map_err(|_| AiError::Subscription("chatgpt_oauth"))?
        .port();
    let pending = oauth::Pending::new(
        format!("http://127.0.0.1:{port}/callback"),
        previous.map(|p| p.client_id),
    )?;
    let url = pending.authorization_url(&protocol, &host)?;
    open_browser(&url).map_err(|_| AiError::Subscription("chatgpt_oauth"))?;
    let code = oauth::loopback_code(listener, &pending, &cancel).await?;
    let tokens = tokio::select! {
        tokens = protocol.exchange(pending, code) => tokens?,
        () = cancel.cancelled() => return Err(AiError::Subscription("chatgpt_cancelled")),
    };
    if cancel.is_cancelled() {
        return Err(AiError::Subscription("chatgpt_cancelled"));
    }
    tokio::task::spawn_blocking(move || store.save(&tokens))
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
    fn chatgpt_credentials_live_only_in_the_dedicated_mock_keychain_entry() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let tokens = Tokens {
            access_token: "fake-access".into(),
            refresh_token: Some("fake-refresh".into()),
            id_token: "fake-id".into(),
            scopes: vec![chatgpt_plan::PLAN_SCOPE.into()],
            expires_at: oauth::now() + 3600,
            client_id: "fake-client".into(),
            subject: "fake-subject".into(),
            display_name: "Test Surveyor".into(),
        };
        PlanKeychain.save(&tokens).unwrap();
        let saved = Entry::new("qiyas", "chatgpt-plan")
            .unwrap()
            .get_password()
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(value["access_token"], "fake-access");
        assert_eq!(
            PlanKeychain.load().unwrap().unwrap().display_name,
            "Test Surveyor"
        );
        assert!(crate::secrets::get_api_key("claude_code").is_err());
        assert!(crate::secrets::get_api_key("chatgpt_plan").is_err());
        PlanKeychain.clear().unwrap();
        assert!(PlanKeychain.load().unwrap().is_none());
        let settings = serde_json::to_string(&crate::settings::Settings::default()).unwrap();
        for secret in ["fake-access", "fake-refresh", "fake-id"] {
            assert!(!settings.contains(secret));
        }
    }
}
