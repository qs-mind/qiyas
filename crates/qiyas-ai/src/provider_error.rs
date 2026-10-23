//! Provider failures have stable codes. The window never receives service diagnostics.

use std::sync::OnceLock;

use serde::Serialize;
use serde_json::Value;

use crate::provider::AiError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicError {
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
    PublicError {
        code: key.clone(),
        message: en[&key]
            .as_str()
            .unwrap_or("The AI service could not complete the request.")
            .into(),
        message_ar: ar[&key].as_str().unwrap_or("تعذّر إكمال الطلب.").into(),
    }
}

impl AiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ChatgptDenied(_) => "chatgpt_denied",
            Self::Subscription(code) => code,
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
                code: "ai.error.chatgpt_denied".into(),
                message: message.clone(),
                message_ar: message.clone(),
            };
        }
        public_error(self.code())
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
            "insufficient balance",
            "billing",
            "payment required",
            "out of credits",
            "spending limit",
        ])
    {
        "billing"
    } else if matches!(status, 401 | 403)
        || has(&["authentication_error", "invalid_api_key", "unauthorized"])
    {
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
    } else if has(&["model_not_found", "not_found_error"])
        || (text.contains("model")
            && has(&[
                "not exist",
                "not found",
                "supported api model names",
                "supported model names",
                "invalid model",
                "unknown model",
            ]))
    {
        "model_not_found"
    } else if has(&["content_filter", "content policy", "safety filter"]) {
        "content_filter"
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
