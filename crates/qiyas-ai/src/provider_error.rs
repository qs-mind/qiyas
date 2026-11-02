//! Provider failures have stable codes. The window never receives service diagnostics.

use std::sync::OnceLock;

use serde::Serialize;
use serde_json::Value;

use crate::provider::AiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    PlanLimit,
    SpendBlocked,
    OutOfCredit,
    RateLimited,
    Auth,
    ModelUnavailable,
    Temporary,
    Offline,
    ContextTooLong,
    Policy,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailureAction {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderFailure {
    pub kind: FailureKind,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    pub actions: Vec<FailureAction>,
}

impl ProviderFailure {
    pub fn from_code(code: &str, provider: &str) -> Self {
        use FailureKind::*;
        let code = code.trim_start_matches("ai.error.");
        let kind = match code {
            "chatgpt_limit" | "claude_limit" | "plan_limit" => PlanLimit,
            "spend_blocked" | "chatgpt_spend_blocked" | "claude_billing" => SpendBlocked,
            "billing" | "out_of_credit" => OutOfCredit,
            "rate_limited" | "chatgpt_rate_limited" => RateLimited,
            "auth"
            | "no_api_key"
            | "chatgpt_auth"
            | "chatgpt_access"
            | "chatgpt_reauthorize"
            | "chatgpt_scope"
            | "chatgpt_denied"
            | "chatgpt_account_mismatch"
            | "claude_auth" => Auth,
            "model_not_found" | "model_capability" | "no_model" | "no_models" | "chatgpt_model"
            | "chatgpt_capability" => ModelUnavailable,
            "timeout" | "stream" | "service_error" | "temporary" | "chatgpt_temporary"
            | "chatgpt_incomplete" | "chatgpt_empty" | "claude_stream" => Temporary,
            "network" | "offline" | "chatgpt_offline" => Offline,
            "context_too_long" => ContextTooLong,
            "content_filter" | "policy" | "chatgpt_policy" | "claude_hold" => Policy,
            _ => Other,
        };
        let provider = if code.starts_with("chatgpt_") {
            "chatgpt_plan"
        } else if code.starts_with("claude_") {
            "claude_code"
        } else {
            provider
        };
        let mut actions = vec![FailureAction {
            kind: "retry",
            page: None,
        }];
        let page = match (provider, kind) {
            ("chatgpt_plan", PlanLimit | SpendBlocked | OutOfCredit) => Some("chatgpt_usage"),
            ("claude_code", PlanLimit | SpendBlocked | OutOfCredit) => Some("claude_usage"),
            ("openai", OutOfCredit | SpendBlocked) => Some("openai_billing"),
            ("anthropic", OutOfCredit | SpendBlocked) => Some("anthropic_billing"),
            _ => None,
        };
        if let Some(page) = page {
            actions.push(FailureAction {
                kind: "open_provider_page",
                page: Some(page),
            });
        }
        if matches!(kind, Auth | ModelUnavailable | OutOfCredit | Other) {
            actions.push(FailureAction {
                kind: "open_settings",
                page: None,
            });
        }
        actions.push(FailureAction {
            kind: "switch_provider",
            page: None,
        });
        Self {
            kind,
            provider: provider.into(),
            resets_at: None,
            retry_after: None,
            actions,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicError {
    #[serde(flatten)]
    pub failure: ProviderFailure,
    pub code: String,
    pub message: String,
    pub message_ar: String,
}

/// Use the same English and Arabic strings as the window.
pub fn public_error(code: &str) -> PublicError {
    static EN: OnceLock<Value> = OnceLock::new();
    static AR: OnceLock<Value> = OnceLock::new();
    let en = EN.get_or_init(|| {
        serde_json::from_str(include_str!("../../../src/i18n/en.json"))
            .expect("English translations")
    });
    let ar = AR.get_or_init(|| {
        serde_json::from_str(include_str!("../../../src/i18n/ar.json"))
            .expect("Arabic translations")
    });
    let key = format!("ai.error.{code}");
    let failure = ProviderFailure::from_code(code, "other");
    let message_key = match failure.kind {
        FailureKind::PlanLimit | FailureKind::SpendBlocked => {
            let kind = if failure.kind == FailureKind::PlanLimit {
                "plan_limit"
            } else {
                "spend_blocked"
            };
            if matches!(failure.provider.as_str(), "chatgpt_plan" | "claude_code") {
                format!("ai.failure.{}.{kind}", failure.provider)
            } else {
                format!("ai.failure.{kind}")
            }
        }
        FailureKind::OutOfCredit => "ai.failure.out_of_credit".into(),
        _ => key.clone(),
    };
    PublicError {
        failure,
        code: key.clone(),
        message: en[&message_key]
            .as_str()
            .unwrap_or("The AI service could not complete the request.")
            .into(),
        message_ar: ar[&message_key]
            .as_str()
            .unwrap_or("تعذر إكمال الطلب.")
            .into(),
    }
}

/// A compatible API is not necessarily an OpenAI account.
pub fn compatible_provider(endpoint: &str) -> &'static str {
    match url::Url::parse(endpoint) {
        Ok(url)
            if url.scheme() == "https"
                && url.host_str() == Some("api.openai.com")
                && url.port_or_known_default() == Some(443) =>
        {
            "openai"
        }
        _ => "other",
    }
}

pub(crate) fn reset_timestamp(detail: &Value) -> Option<u64> {
    detail
        .get("resets_at")
        .or_else(|| detail.get("reset_at"))
        .and_then(|value| {
            value
                .as_u64()
                .map(|n| if n >= 1_000_000_000_000 { n / 1000 } else { n })
                .or_else(|| {
                    value
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .and_then(|d| u64::try_from(d.timestamp()).ok())
                })
        })
}

impl AiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ChatgptDenied(_) => "chatgpt_denied",
            Self::Subscription(code) | Self::SubscriptionFailure { code, .. } => code,
            Self::ModelNotFound { .. } => "model_not_found",
            Self::Auth => "auth",
            Self::RateLimited { .. } => "rate_limited",
            Self::Timeout(_) => "timeout",
            Self::Network(_) => "network",
            Self::Api {
                status,
                message,
                body,
                ..
            } => classify(*status, &format!("{message} {body}")),
            Self::Stream(message) => {
                let code = classify(0, message);
                if code == "service_error"
                    && ![
                        "overloaded",
                        "server_error",
                        "api_error",
                        "temporarily unavailable",
                    ]
                    .iter()
                    .any(|word| message.to_ascii_lowercase().contains(word))
                {
                    "stream"
                } else {
                    code
                }
            }
            Self::Server(_) => "service_error",
        }
    }

    pub fn public_error(&self) -> PublicError {
        if let Self::ChatgptDenied(message) = self {
            return PublicError {
                failure: ProviderFailure::from_code("chatgpt_denied", "chatgpt_plan"),
                code: "ai.error.chatgpt_denied".into(),
                message: message.clone(),
                message_ar: message.clone(),
            };
        }
        let mut public = public_error(self.code());
        match self {
            Self::SubscriptionFailure {
                resets_at,
                retry_after,
                ..
            } => {
                public.failure.resets_at = *resets_at;
                public.failure.retry_after = *retry_after;
            }
            Self::RateLimited { retry_after } => public.failure.retry_after = *retry_after,
            _ => {}
        }
        public
    }

    pub fn public_error_for(&self, provider: &str) -> PublicError {
        let mut public = self.public_error();
        let mut failure = ProviderFailure::from_code(self.code(), provider);
        failure.resets_at = public.failure.resets_at;
        failure.retry_after = public.failure.retry_after;
        public.failure = failure;
        public
    }

    pub(crate) fn with_retry_after(self, retry_after: Option<u64>) -> Self {
        match (self, retry_after) {
            (Self::Subscription(code), Some(seconds)) => Self::SubscriptionFailure {
                code,
                resets_at: None,
                retry_after: Some(seconds),
            },
            (
                Self::SubscriptionFailure {
                    code, resets_at, ..
                },
                Some(seconds),
            ) => Self::SubscriptionFailure {
                code,
                resets_at,
                retry_after: Some(seconds),
            },
            (error, _) => error,
        }
    }

    pub fn for_window_for(&self, provider: &str) -> String {
        serde_json::to_string(&self.public_error_for(provider)).expect("plain provider error")
    }

    /// String envelope preserves the existing IPC error contract and saved conversations.
    pub fn for_window(&self) -> String {
        serde_json::to_string(&self.public_error()).expect("plain provider error")
    }

    pub(crate) fn retryable(&self) -> bool {
        if matches!(
            self.code(),
            "billing"
                | "auth"
                | "context_too_long"
                | "model_not_found"
                | "output_limit"
                | "model_capability"
                | "content_filter"
        ) {
            return false;
        }
        match self {
            Self::Network(_) | Self::Timeout(_) | Self::RateLimited { .. } => true,
            Self::Api { status, .. } => *status == 429 || (500..600).contains(status),
            Self::Stream(message) => transient_stream(message),
            _ => false,
        }
    }
}

fn transient_stream(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "overloaded",
        "server_error",
        "api_error",
        "temporarily unavailable",
        "connection reset",
        "connection closed",
        "error decoding response body",
    ]
    .iter()
    .any(|s| text.contains(s))
}

fn classify(status: u16, text: &str) -> &'static str {
    let text = text.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
    // Billing takes precedence over status, including 403 and 429 quota failures.
    if status == 402
        || has(&[
            "quota",
            "insufficient credit",
            "insufficient_credit",
            "not enough credits",
            "credit balance",
            "credit_balance_exhausted",
            "spend_limit_exceeded",
            "usage_limit_exceeded",
            "insufficient balance",
            "billing",
            "payment required",
            "out of credits",
            "spending limit",
        ])
    {
        "billing"
    } else if status == 401 || has(&["authentication_error", "invalid_api_key", "unauthorized"]) {
        "auth"
    } else if has(&[
        "context_length_exceeded",
        "maximum context length",
        "context window",
        "prompt is too long",
        "input is too long",
        "too many input tokens",
    ]) {
        "context_too_long"
    } else if has(&["model_not_found", "model_not_allowed", "not_found_error"])
        || (text.contains("model")
            && has(&[
                "not exist",
                "not found",
                "supported api model names",
                "supported model names",
                "invalid model",
                "unknown model",
                "do not have access",
                "not allowed",
            ]))
    {
        "model_not_found"
    } else if has(&["content_filter", "content policy", "safety filter"]) {
        "content_filter"
    } else if status == 403 {
        "auth"
    } else if has(&["max_tokens", "max_completion_tokens"]) {
        "output_limit"
    } else if (has(&["image", "vision", "tool"])) && has(&["not support", "unsupported"]) {
        "model_capability"
    } else if status == 429 || has(&["rate_limit", "rate limit"]) {
        "rate_limited"
    } else {
        "service_error"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn billing_links_are_only_for_the_actual_api_host() {
        for (endpoint, expected) in [
            ("https://api.openai.com/v1", "openai"),
            ("https://api.openai.com.evil.example/v1", "other"),
            ("http://localhost:8080/v1", "other"),
            ("https://api.openai.com:444/v1", "other"),
        ] {
            assert_eq!(compatible_provider(endpoint), expected);
        }
    }

    #[test]
    fn unavailable_model_permissions_are_not_an_auth_problem() {
        for text in ["model_not_allowed", "You do not have access to this model"] {
            let failure = crate::provider::response_error(403, text, &Value::Null, "");
            assert_eq!(
                failure.public_error().failure.kind,
                FailureKind::ModelUnavailable
            );
        }
    }

    #[test]
    fn api_spend_limits_and_balance_codes_are_not_temporary_rate_limits() {
        for code in [
            "credit_balance_exhausted",
            "organization_spend_limit_exceeded",
            "project_spend_limit_exceeded",
            "organization_usage_limit_exceeded",
        ] {
            let error = crate::provider::response_error(429, code, &Value::Null, "");
            assert_eq!(
                error.public_error_for("openai").failure.kind,
                FailureKind::OutOfCredit
            );
            assert!(!error.retryable());
        }
    }

    #[test]
    fn shared_kinds_preserve_provider_specific_recovery() {
        for (provider, status, body, kind, page) in [
            (
                "openai",
                429,
                "insufficient_quota",
                "out_of_credit",
                Some("openai_billing"),
            ),
            ("openai", 429, "rate_limit_exceeded", "rate_limited", None),
            ("anthropic", 529, "overloaded_error", "temporary", None),
            (
                "anthropic",
                400,
                "credit balance is too low",
                "out_of_credit",
                Some("anthropic_billing"),
            ),
            ("openai", 401, "invalid_api_key", "auth", None),
            ("openai", 403, "model_not_found", "model_unavailable", None),
        ] {
            let error =
                crate::provider::response_error(status, body, &serde_json::json!({}), "secret");
            let value = serde_json::to_value(error.public_error_for(provider)).unwrap();
            assert_eq!(value["kind"], kind);
            assert_eq!(value["provider"], provider);
            assert!(value["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["kind"] == "retry"));
            if let Some(page) = page {
                assert!(value["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["page"] == page));
            }
        }
        let limited = AiError::RateLimited {
            retry_after: Some(45),
        }
        .public_error_for("openai");
        assert_eq!(serde_json::to_value(limited).unwrap()["retry_after"], 45);
        for (code, kind) in [
            ("chatgpt_limit", "plan_limit"),
            ("claude_limit", "plan_limit"),
            ("claude_billing", "spend_blocked"),
            ("network", "offline"),
        ] {
            assert_eq!(
                serde_json::to_value(public_error(code)).unwrap()["kind"],
                kind
            );
        }
    }

    #[test]
    fn billing_never_retries_even_when_the_status_says_busy() {
        for status in [400, 402, 403, 429, 503] {
            for text in [
                "quota exceeded for customer_id=internal",
                "insufficient_quota",
                "Insufficient credits",
                "credit balance is too low",
                "billing_hard_limit_reached",
            ] {
                let error = AiError::Api {
                    status,
                    message: text.into(),
                    body: text.into(),
                    request: None,
                };
                assert_eq!(error.code(), "billing");
                assert!(!error.retryable());
                let public = error.for_window();
                assert!(!public.contains(text));
                assert!(public.contains("ai.error.billing"));
            }
        }
    }

    #[test]
    fn value_limits_and_context_limits_are_distinct() {
        assert_eq!(classify(404, "route not found"), "service_error");
        assert_eq!(
            AiError::Stream("the connection closed before the answer finished".into()).code(),
            "stream"
        );
        assert_eq!(
            classify(400, "max_tokens must be between 1 and 8192"),
            "output_limit"
        );
        assert_eq!(
            classify(400, "maximum context length exceeded, including max_tokens"),
            "context_too_long"
        );
        assert_eq!(
            classify(
                400,
                "The supported API model names are deepseek-flash, but you passed another"
            ),
            "model_not_found"
        );
    }
}
