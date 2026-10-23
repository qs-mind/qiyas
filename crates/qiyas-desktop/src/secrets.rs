//! The AI provider's API key, kept in the OS keychain only: service `qiyas`, account = provider
//! and normalized base URL. Legacy provider-only entries are migration sources only.
//! Never written to a file, logged or sent back to the window.
//!
//! The functions use whatever `keyring_core` default store is set: the app sets the OS one with
//! `use_native_store` at startup; tests set the mock store.

use keyring_core::{Entry, Error};

use crate::settings::Provider;

const SERVICE: &str = "qiyas";

/// Sets the OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux) as
/// the store. On failure the app still runs; saving a key then says there is no keychain.
pub fn use_native_store() -> Result<(), String> {
    keyring::Entry::store_status()
        .as_ref()
        .map_err(|e| format!("No keychain available: {e}"))?;
    Ok(())
}

fn entry(provider: &str) -> Result<Entry, String> {
    if !matches!(
        Provider::from_name(provider),
        Some(Provider::Anthropic | Provider::OpenaiCompatible)
    ) {
        return Err(format!("Unknown provider \"{provider}\""));
    }
    Entry::new(SERVICE, provider).map_err(describe)
}

fn describe(e: Error) -> String {
    match e {
        Error::NoDefaultStore | Error::NoStorageAccess(_) | Error::PlatformFailure(_) => {
            format!("No keychain available: {e}")
        }
        e => format!("Keychain error: {e}"),
    }
}

/// Stores `key` (trimmed) for `provider`, replacing any key already there.
pub fn set_api_key(provider: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("The API key is empty".into());
    }
    entry(provider)?.set_password(key).map_err(describe)
}

/// The key stored for `provider`, or None when there is none.
pub fn get_api_key(provider: &str) -> Result<Option<String>, String> {
    match entry(provider)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(describe(e)),
    }
}

/// Removes the key for `provider`; removing a key that isn't there is fine.
pub fn delete_api_key(provider: &str) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(describe(e)),
    }
}

/// Complete saved address, including its path, scopes every credential operation.
pub fn address(settings: &crate::settings::Settings, provider: &str) -> Result<String, String> {
    match Provider::from_name(provider) {
        Some(Provider::Anthropic) => Ok("https://api.anthropic.com/v1".into()),
        Some(Provider::OpenaiCompatible) => qiyas_ai::endpoint::normalize(
            settings
                .providers
                .openai_compatible
                .base_url
                .as_deref()
                .ok_or("Save the service address first")?,
        ),
        _ => Err("This provider does not use an API key".into()),
    }
}
fn scoped(provider: &str, base: &str) -> Result<Entry, String> {
    if !matches!(
        Provider::from_name(provider),
        Some(Provider::Anthropic | Provider::OpenaiCompatible)
    ) {
        return Err("Unknown provider".into());
    }
    let base = qiyas_ai::endpoint::normalize(base)?;
    Entry::new(SERVICE, &format!("{provider}:{base}")).map_err(describe)
}
pub fn get_at(provider: &str, base: &str) -> Result<Option<String>, String> {
    match scoped(provider, base)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(describe(e)),
    }
}
pub fn set_at(provider: &str, base: &str, key: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("The API key is empty".into());
    }
    scoped(provider, base)?
        .set_password(key.trim())
        .map_err(describe)
}
pub fn delete_at(provider: &str, base: &str) -> Result<(), String> {
    match scoped(provider, base)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(describe(e)),
    }
}
/// Run before changing saved settings. A legacy key can migrate only to its saved address.
pub fn migrate(settings: &crate::settings::Settings) -> Result<(), String> {
    for provider in ["anthropic", "openai_compatible"] {
        if let Some(key) = get_api_key(provider)? {
            let base = address(settings, provider)?;
            if get_at(provider, &base)?.is_none() {
                set_at(provider, &base, &key)?;
            }
            delete_api_key(provider)?;
        }
    }
    Ok(())
}
pub fn get_saved(
    settings: &crate::settings::Settings,
    provider: &str,
) -> Result<Option<String>, String> {
    let base = address(settings, provider)?;
    migrate(settings)?;
    get_at(provider, &base)
}

/// Reject draft destinations before reading any saved credential.
pub fn discovery_address(
    settings: &crate::settings::Settings,
    provider: &str,
    requested: Option<&str>,
) -> Result<String, String> {
    let saved = address(settings, provider)?;
    let requested = requested
        .map(qiyas_ai::endpoint::normalize)
        .transpose()?
        .unwrap_or_else(|| saved.clone());
    if requested != saved {
        return Err("save_address_first".into());
    }
    Ok(saved)
}
