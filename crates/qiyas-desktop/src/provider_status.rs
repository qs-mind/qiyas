//! Local configuration and the last observed provider result. Never probes a model or
//! exposes credentials, account subjects, diagnostics, or user-supplied URLs over IPC.
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use qiyas_ai::provider_error::{FailureAction, FailureKind, PublicError};
use serde::Serialize;
use tauri::State;

use crate::settings::{Provider, Settings};
use crate::state::{lock, AppState};

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderIdentity {
    provider: &'static str,
    profile: u64,
    generation: u64,
}
#[derive(Default)]
struct Observations {
    generations: Vec<(&'static str, u64)>,
    results: Vec<(ProviderIdentity, Option<PublicError>)>,
    claude: Vec<(Option<String>, bool, Option<bool>)>,
}
static OBSERVED: Mutex<Observations> = Mutex::new(Observations {
    generations: Vec::new(),
    results: Vec::new(),
    claude: Vec::new(),
});

impl Observations {
    fn generation(&self, provider: &str) -> u64 {
        self.generations
            .iter()
            .find(|(p, _)| *p == provider)
            .map_or(0, |(_, g)| *g)
    }
    fn record(&mut self, identity: &ProviderIdentity, failure: Option<PublicError>) {
        if identity.generation != self.generation(identity.provider) {
            return;
        }
        self.results.retain(|(key, _)| key != identity);
        if self.results.len() >= 32 {
            self.results.remove(0);
        }
        self.results.push((identity.clone(), failure));
    }
    fn claude_configured(&self, executable: Option<&str>) -> bool {
        self.claude
            .iter()
            .find(|(path, _, _)| path.as_deref() == executable)
            .is_none_or(|(_, installed, signed_in)| *installed && *signed_in != Some(false))
    }
    fn record_claude(
        &mut self,
        executable: Option<&str>,
        installed: bool,
        signed_in: Option<bool>,
    ) {
        let next = (executable.map(str::to_owned), installed, signed_in);
        let changed = self
            .claude
            .iter()
            .find(|(path, _, _)| path.as_deref() == executable)
            != Some(&next);
        if changed {
            self.clear("claude_code");
        }
        self.claude
            .retain(|(path, _, _)| path.as_deref() != executable);
        if self.claude.len() >= 32 {
            self.claude.remove(0);
        }
        self.claude.push(next);
    }
    fn clear(&mut self, provider: &'static str) {
        let next = self.generation(provider).wrapping_add(1);
        self.generations.retain(|(p, _)| *p != provider);
        self.generations.push((provider, next));
        self.results.retain(|(key, _)| key.provider != provider);
    }
}

/// Cache only the explicit CLI status check. Never inspects Claude credential storage.
pub fn record_claude_status(executable: Option<&str>, status: &qiyas_ai::claude_code::Status) {
    lock(&OBSERVED).record_claude(executable, status.installed, status.signed_in);
}

pub fn provider_name(settings: &Settings) -> &'static str {
    match settings.provider {
        Provider::ChatgptPlan => "chatgpt_plan",
        Provider::ClaudeCode => "claude_code",
        Provider::Anthropic => "anthropic",
        Provider::OpenaiCompatible => qiyas_ai::provider_error::compatible_provider(
            settings.profile().base_url.as_deref().unwrap_or(""),
        ),
    }
}

fn profile_identity(
    settings: &Settings,
    credential: impl Hash,
    generation: u64,
) -> ProviderIdentity {
    let mut hash = DefaultHasher::new();
    settings.provider.name().hash(&mut hash);
    settings.profile().base_url.hash(&mut hash);
    settings.profile().model.hash(&mut hash);
    settings.claude_executable.hash(&mut hash);
    credential.hash(&mut hash);
    ProviderIdentity {
        provider: provider_name(settings),
        profile: hash.finish(),
        generation,
    }
}

struct LocalStatus {
    identity: ProviderIdentity,
    configured: bool,
    failure: Option<PublicError>,
}
async fn local_status(settings: &Settings) -> Result<LocalStatus, String> {
    let provider = provider_name(settings);
    let generation = lock(&OBSERVED).generation(provider);
    if settings.provider == Provider::ChatgptPlan {
        let account = qiyas_ai::chatgpt_plan::account(Arc::new(crate::subscriptions::PlanKeychain))
            .await
            .map_err(|e| e.for_window())?;
        let identity = profile_identity(
            settings,
            (&account.active_id, account.signed_in),
            generation,
        );
        let failure = if account.paused {
            let mut error =
                qiyas_ai::AiError::Subscription("chatgpt_limit").public_error_for(provider);
            error.failure.resets_at = account.resets_at;
            Some(error)
        } else if account.signed_in && !account.plan_granted {
            Some(qiyas_ai::AiError::Subscription("chatgpt_scope").public_error_for(provider))
        } else {
            None
        };
        return Ok(LocalStatus {
            identity,
            configured: account.signed_in,
            failure,
        });
    }
    if settings.provider == Provider::ClaudeCode {
        // Claude owns sign-in. The existing explicit status command checks its CLI;
        // this local status command must never launch it or read its credential files.
        return Ok(LocalStatus {
            identity: profile_identity(settings, (), generation),
            configured: lock(&OBSERVED).claude_configured(settings.claude_executable.as_deref()),
            failure: None,
        });
    }
    let saved = settings.clone();
    let key = tokio::task::spawn_blocking(move || {
        crate::secrets::get_saved(&saved, saved.provider.name())
    })
    .await
    .ok()
    .and_then(Result::ok)
    .flatten();
    let local_endpoint = settings.provider == Provider::OpenaiCompatible
        && settings.profile().base_url.as_deref().is_some_and(|base| {
            qiyas_ai::endpoint::normalize(base).is_ok()
                && ["http://localhost", "http://127.0.0.1", "http://[::1]"]
                    .iter()
                    .any(|host| {
                        base.strip_prefix(host).is_some_and(|tail| {
                            tail.is_empty() || tail.starts_with(':') || tail.starts_with('/')
                        })
                    })
        });
    Ok(LocalStatus {
        identity: profile_identity(settings, &key, generation),
        configured: key.is_some() || local_endpoint,
        failure: None,
    })
}

/// Capture before starting a request; record with that same identity after it finishes.
pub async fn identity(settings: &Settings) -> Result<ProviderIdentity, String> {
    Ok(local_status(settings).await?.identity)
}
pub fn record_result(identity: &ProviderIdentity, failure: Option<PublicError>) {
    lock(&OBSERVED).record(identity, failure);
}

/// Draft destinations and keys never change the saved profile's last observed result.
pub fn tracks_discovery(
    settings: &Settings,
    provider: &str,
    requested: &str,
    draft_key: bool,
) -> bool {
    !draft_key
        && settings.provider.name() == provider
        && crate::secrets::address(settings, provider).is_ok_and(|saved| saved == requested)
}

fn window_failure(provider: &str, envelope: &str) -> Option<PublicError> {
    let value: serde_json::Value = serde_json::from_str(envelope).ok()?;
    let code = value.get("code")?.as_str()?.strip_prefix("ai.error.")?;
    if code.is_empty()
        || code.len() > 64
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    {
        return None;
    }
    // Model discovery exposes only the existing serialized public envelope. Rebuild all
    // display text and actions; never carry raw diagnostic text or arbitrary links across.
    let mut error = qiyas_ai::provider_error::public_error(code);
    error.failure = qiyas_ai::provider_error::ProviderFailure::from_code(code, provider);
    error.failure.resets_at = value.get("resets_at").and_then(serde_json::Value::as_u64);
    error.failure.retry_after = value.get("retry_after").and_then(serde_json::Value::as_u64);
    if let Some(reason) = value
        .pointer("/details/reason")
        .and_then(serde_json::Value::as_str)
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 80
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
    {
        error.failure.details = Some(serde_json::json!({"reason":reason}));
    }
    Some(error)
}

/// Use the identity captured before model discovery starts. Legacy/raw errors are ignored;
/// a built-in fallback list must not call this with None and claim a successful connection.
pub fn record_window_result(identity: &ProviderIdentity, error: Option<&str>) {
    match error {
        Some(envelope) => {
            if let Some(failure) = window_failure(identity.provider, envelope) {
                record_result(identity, Some(failure));
            }
        }
        None => record_result(identity, None),
    }
}
/// Explicit account transitions invalidate even an in-flight result from the old session.
pub fn clear_provider(provider: &'static str) {
    lock(&OBSERVED).clear(provider);
}

#[derive(Serialize)]
pub struct ProviderStatus {
    provider: &'static str,
    status: &'static str,
    configured: bool,
    /// True only after a successful provider request in this app session and profile.
    verified: bool,
    last_failure: Option<PublicError>,
    actions: Vec<FailureAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resets_at: Option<u64>,
}
fn status_from(local: LocalStatus, observed: Option<Option<PublicError>>) -> ProviderStatus {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    status_from_at(local, observed, now)
}
fn status_from_at(
    local: LocalStatus,
    observed: Option<Option<PublicError>>,
    now: u64,
) -> ProviderStatus {
    let verified = matches!(observed, Some(None)) && local.failure.is_none() && local.configured;
    let failure = local
        .failure
        .or_else(|| observed.flatten())
        .filter(|error| {
            !(error.failure.kind == FailureKind::PlanLimit
                && error.failure.resets_at.is_some_and(|reset| reset <= now))
        });
    let status = match failure.as_ref().map(|failure| failure.failure.kind) {
        Some(FailureKind::PlanLimit) => "plan_limit",
        Some(FailureKind::SpendBlocked) => "spend_blocked",
        Some(FailureKind::OutOfCredit) => "out_of_credit",
        Some(FailureKind::Auth) => "auth",
        _ if !local.configured => "needs_sign_in",
        _ if verified => "ok",
        _ => "connected",
    };
    ProviderStatus {
        provider: local.identity.provider,
        status,
        configured: local.configured,
        verified,
        actions: failure.as_ref().map_or_else(
            || {
                if local.configured {
                    Vec::new()
                } else {
                    vec![
                        FailureAction {
                            kind: "open_settings",
                            page: None,
                        },
                        FailureAction {
                            kind: "switch_provider",
                            page: None,
                        },
                    ]
                }
            },
            |f| f.failure.actions.clone(),
        ),
        resets_at: failure.as_ref().and_then(|f| f.failure.resets_at),
        last_failure: failure,
    }
}

#[tauri::command]
pub async fn ai_provider_status(
    workspace: Option<u32>,
    state: State<'_, AppState>,
) -> Result<ProviderStatus, String> {
    let state = state.scoped(workspace)?;
    let settings = lock(&state.provider).clone();
    let local = local_status(&settings).await?;
    let observed = lock(&OBSERVED)
        .results
        .iter()
        .find(|(key, _)| key == &local.identity)
        .map(|(_, result)| result.clone());
    Ok(status_from(local, observed))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn local(configured: bool) -> LocalStatus {
        LocalStatus {
            identity: profile_identity(&Settings::default(), (), 0),
            configured,
            failure: None,
        }
    }
    #[test]
    fn discovery_failures_rebuild_public_text_and_actions_from_the_code() {
        let envelope = serde_json::json!({
            "code": "ai.error.out_of_credit", "message": "secret credential diagnostics",
            "message_ar": "secret", "provider": "malicious", "kind": "temporary",
            "resets_at": 300, "retry_after": 20,
            "actions": [{"kind": "open_provider_page", "page": "https://evil.example"}]
        })
        .to_string();
        let error = window_failure("openai", &envelope).unwrap();
        assert_eq!(error.failure.kind, FailureKind::OutOfCredit);
        assert_eq!(error.failure.provider, "openai");
        assert_eq!(error.failure.resets_at, Some(300));
        assert_eq!(error.failure.retry_after, Some(20));
        let public = serde_json::to_string(&error).unwrap();
        assert!(!public.contains("secret"));
        assert!(!public.contains("evil.example"));
        assert!(error
            .failure
            .actions
            .iter()
            .any(|action| action.page == Some("openai_billing")));
        assert!(window_failure("openai", "raw error containing credentials").is_none());
        assert!(window_failure("openai", r#"{"code":"ai.error.secret credential"}"#).is_none());
    }
    #[test]
    fn incomplete_reason_survives_the_saved_failure_model() {
        let error = window_failure(
            "chatgpt_plan",
            r#"{"code":"ai.error.content_filter","details":{"reason":"content_filter"}}"#,
        )
        .unwrap();
        assert_eq!(error.failure.details.unwrap()["reason"], "content_filter");
    }
    #[test]
    fn discovery_tracks_only_the_saved_selected_provider_without_a_draft_key() {
        let mut settings = Settings {
            provider: Provider::OpenaiCompatible,
            ..Settings::default()
        };
        settings.profile_mut().base_url = Some("https://api.openai.com/v1".into());
        assert!(tracks_discovery(
            &settings,
            "openai_compatible",
            "https://api.openai.com/v1",
            false
        ));
        assert!(!tracks_discovery(
            &settings,
            "openai_compatible",
            "https://api.openai.com/v1",
            true
        ));
        assert!(!tracks_discovery(
            &settings,
            "openai_compatible",
            "https://other.example/v1",
            false
        ));
        assert!(!tracks_discovery(
            &settings,
            "anthropic",
            "https://api.anthropic.com/v1",
            false
        ));
    }
    #[test]
    fn expired_plan_limits_clear_without_claiming_verified() {
        let mut error =
            qiyas_ai::AiError::Subscription("chatgpt_limit").public_error_for("chatgpt_plan");
        error.failure.resets_at = Some(200);
        assert_eq!(
            status_from_at(local(true), Some(Some(error.clone())), 199).status,
            "plan_limit"
        );
        let status = status_from_at(local(true), Some(Some(error)), 200);
        assert_eq!(status.status, "connected");
        assert!(!status.verified);
        assert!(status.last_failure.is_none());
    }
    #[test]
    fn claude_status_uses_only_explicit_checks_and_tracks_executable_changes() {
        let mut observations = Observations::default();
        assert!(observations.claude_configured(None)); // Unknown, never verified.
        observations.record_claude(None, true, Some(false));
        assert!(!observations.claude_configured(None));
        assert!(observations.claude_configured(Some("/other/claude")));
        observations.record_claude(None, true, Some(true));
        assert!(observations.claude_configured(None));
        observations.record_claude(None, false, None);
        assert!(!observations.claude_configured(None));
        assert_eq!(
            status_from(local(false), None).actions[0].kind,
            "open_settings"
        );
    }
    #[test]
    fn configured_does_not_claim_a_verified_connection() {
        let status = status_from(local(true), None);
        assert_eq!(status.status, "connected");
        assert!(!status.verified);
        assert_eq!(status_from(local(false), None).status, "needs_sign_in");
        assert!(status_from(local(true), Some(None)).verified);
    }
    #[test]
    fn errors_keep_classification_and_actions_until_success() {
        for (code, expected) in [
            ("chatgpt_limit", "plan_limit"),
            ("spend_blocked", "spend_blocked"),
            ("out_of_credit", "out_of_credit"),
            ("auth", "auth"),
        ] {
            let error = qiyas_ai::AiError::Subscription(code).public_error_for("openai");
            let status = status_from(local(true), Some(Some(error.clone())));
            assert_eq!(status.status, expected);
            assert_eq!(status.actions, error.failure.actions);
            assert!(!status.verified);
        }
        assert_eq!(status_from(local(true), Some(None)).status, "ok");
    }
    #[test]
    fn profile_account_and_credential_changes_do_not_inherit_failures() {
        let mut settings = Settings::default();
        let first = profile_identity(&settings, "account-a", 0);
        assert!(first != profile_identity(&settings, "account-b", 0));
        settings.profile_mut().base_url = Some("https://another.example/v1".into());
        assert!(first != profile_identity(&settings, "account-a", 0));
        let mut observations = Observations::default();
        observations.record(&first, None);
        observations.clear(first.provider);
        observations.record(&first, None);
        assert!(
            observations.results.is_empty(),
            "old in-flight response must stay invalid"
        );
    }
    #[test]
    fn only_openai_host_gets_openai_billing_actions() {
        let mut settings = Settings {
            provider: Provider::OpenaiCompatible,
            ..Settings::default()
        };
        for (url, provider) in [
            ("https://api.openai.com/v1", "openai"),
            ("https://api.openai.com.evil.example/v1", "other"),
            ("https://third-party.example/v1", "other"),
        ] {
            settings.profile_mut().base_url = Some(url.into());
            assert_eq!(provider_name(&settings), provider);
        }
    }
}
