//! Background detection never holds up a drawing turn.
use std::collections::HashSet;
use std::hash::Hasher;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use qiyas_ai::pictures::{Cache, Detected, Key};
use qiyas_ai::Provider;
use tauri::State;

use crate::settings::{self, Settings};
use crate::state::{lock, AppState};

static CACHE: OnceLock<Arc<Cache>> = OnceLock::new();
static PENDING: OnceLock<Mutex<HashSet<Key>>> = OnceLock::new();
fn cache() -> Result<Arc<Cache>, String> {
    if let Some(cache) = CACHE.get() {
        return Ok(cache.clone());
    }
    let path = qiyas_core::data_home::qiyas_dir()?.join("picture-support.json");
    Ok(CACHE.get_or_init(|| Arc::new(Cache::new(path))).clone())
}
async fn key(settings: &Settings) -> Result<Key, String> {
    let scope = match settings.provider {
        settings::Provider::ChatgptPlan => {
            qiyas_ai::chatgpt_plan::account(Arc::new(crate::subscriptions::PlanKeychain))
                .await
                .map_err(|e| e.for_window())?
                .active_id
                .ok_or("chatgpt_no_account")?
        }
        settings::Provider::ClaudeCode => settings
            .claude_executable
            .clone()
            .unwrap_or_else(|| "claude".into()),
        settings::Provider::Anthropic => qiyas_ai::anthropic::API_URL.into(),
        settings::Provider::OpenaiCompatible => {
            qiyas_ai::endpoint::normalize(settings.profile().base_url.as_deref().unwrap_or(""))?
        }
    };
    let mut hash = fnv::FnvHasher::default();
    hash.write(scope.as_bytes());
    Ok(Key {
        provider: settings.provider.name().into(),
        scope: format!("{:016x}", hash.finish()),
        model: settings.profile().model.clone(),
    })
}
pub fn selected(state: AppState, settings: Settings) {
    tauri::async_runtime::spawn(async move {
        if let (Ok(identity), Ok(cache)) = (key(&settings).await, cache()) {
            schedule(state, settings, None, cache, identity);
        }
    });
}
pub async fn known(state: &AppState, settings: &Settings) -> Option<bool> {
    let cache = cache().ok()?;
    let identity = key(settings).await.ok()?;
    let answer = cache.get(&identity);
    schedule(state.clone(), settings.clone(), None, cache, identity);
    answer
}
pub async fn wrap(
    state: &AppState,
    settings: &Settings,
    provider: Box<dyn Provider>,
) -> Result<Box<dyn Provider>, String> {
    let key = key(settings).await?;
    let cache = cache()?;
    schedule(
        state.clone(),
        settings.clone(),
        None,
        cache.clone(),
        key.clone(),
    );
    Ok(Box::new(Detected::new(provider, cache, key)))
}

async fn provider(
    state: &AppState,
    settings: &Settings,
    typed_key: Option<String>,
) -> Result<Box<dyn Provider>, String> {
    match settings.provider {
        settings::Provider::ChatgptPlan => crate::subscriptions::provider(),
        settings::Provider::ClaudeCode => Ok(Box::new(
            qiyas_ai::claude_code::ClaudeCode::new(
                settings.claude_executable.as_deref(),
                qiyas_core::data_home::qiyas_dir()?,
                state.session.clone(),
            )
            .map_err(|e| e.for_window())?,
        )),
        _ => {
            let saved = settings.clone();
            let key = match typed_key {
                Some(key) => Some(key),
                None => match tokio::task::spawn_blocking(move || {
                    crate::secrets::get_saved(&saved, saved.provider.name())
                })
                .await
                .map_err(|e| e.to_string())?
                {
                    Ok(key) => key,
                    Err(_) if settings.provider == settings::Provider::OpenaiCompatible => None,
                    Err(error) => return Err(error),
                },
            };
            crate::ai::build_provider(settings, key)
        }
    }
}
fn busy(state: &AppState) -> bool {
    state
        .conversations()
        .iter()
        .any(|workspace| lock(&workspace.ai).busy())
}
fn schedule(
    state: AppState,
    settings: Settings,
    typed_key: Option<String>,
    cache: Arc<Cache>,
    identity: Key,
) {
    #[cfg(test)]
    if lock(&state.ai).test_provider.is_some() {
        return;
    }
    if crate::ai::mock_provider(&state.session).is_some() || !cache.due(&identity) {
        return;
    }
    let pending = PENDING.get_or_init(Mutex::default);
    if !lock(pending).insert(identity.clone()) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        // First use waits until the turn finishes. Starting a turn cancels a probe.
        while busy(&state) {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if key(&settings).await.ok().as_ref() == Some(&identity) {
            if let Ok(provider) = provider(&state, &settings, typed_key).await {
                let detection = qiyas_ai::pictures::detect(&cache, &identity, provider.as_ref());
                tokio::pin!(detection);
                loop {
                    tokio::select! {
                        _ = &mut detection => break,
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {
                            if busy(&state) { break; }
                        }
                    }
                }
            }
        }
        lock(pending).remove(&identity);
    });
}

/// Draft addresses never receive a saved credential.
#[tauri::command]
pub async fn ai_picture_support(
    provider: settings::Provider,
    model: String,
    base_url: Option<String>,
    typed_key: Option<String>,
    executable: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<bool>, String> {
    let mut settings = lock(&state.settings).clone();
    let typed_key = typed_key.filter(|key| !key.trim().is_empty());
    let base = if provider == settings::Provider::OpenaiCompatible {
        Some(if typed_key.is_some() {
            qiyas_ai::endpoint::normalize(base_url.as_deref().unwrap_or(""))?
        } else {
            crate::secrets::discovery_address(&settings, provider.name(), base_url.as_deref())?
        })
    } else {
        None
    };
    settings.provider = provider;
    settings.profile_mut().model = model;
    if let Some(base) = base {
        settings.profile_mut().base_url = Some(base);
    }
    if let Some(executable) = executable {
        settings.claude_executable = (!executable.trim().is_empty()).then_some(executable);
    }
    let identity = key(&settings).await?;
    let cache = cache()?;
    let answer = cache.get(&identity);
    schedule(state.inner().clone(), settings, typed_key, cache, identity);
    Ok(answer)
}
