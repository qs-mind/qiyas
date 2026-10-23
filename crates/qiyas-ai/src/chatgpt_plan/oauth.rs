//! OAuth and OIDC primitives. Endpoints are fixed by the reviewed SIWC protocol, never by
//! Settings or an ID token's untrusted jku/x5u headers.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::rand::SecureRandom;
use ring::{digest, rand, signature};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use super::{Tokens, PLAN_SCOPE};
use crate::AiError;

#[derive(Clone)]
pub struct Protocol {
    pub authorization: String,
    pub token: String,
    pub issuer: String,
    pub discovery: String,
    pub responses: String,
    pub models: String,
}

fn invalid() -> AiError {
    AiError::Subscription("chatgpt_oauth")
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub const RESOURCE: &str = "https://api.openai.com/v1";

impl Protocol {
    /// The reviewed SIWC endpoints. Tests supply only local mock endpoints.
    pub fn official() -> Self {
        Self {
            authorization: "https://auth.openai.com/api/accounts/authorize".into(),
            token: "https://auth.openai.com/api/accounts/oauth/token".into(),
            issuer: "https://auth.openai.com".into(),
            discovery: "https://auth.openai.com/.well-known/openid-configuration".into(),
            responses: format!("{RESOURCE}/responses"),
            models: format!("{RESOURCE}/models"),
        }
    }
}

/// Codes and registration IDs stay in memory until the verified identity is saved.
pub struct Grant {
    pub code: String,
    pub client_id: String,
}

fn issued_client(value: &str) -> bool {
    value.starts_with("oaiapp_")
        && value.len() > 7
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn random() -> Result<String, AiError> {
    let mut bytes = [0u8; 32];
    rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| invalid())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Kept only in memory during a sign-in; never serialize or Debug this structure.
pub struct Pending {
    pub(crate) verifier: String,
    pub(crate) state: String,
    pub(crate) nonce: String,
    pub(crate) redirect: String,
    pub(crate) client_id: String,
}

impl Pending {
    pub fn new(redirect: String, client_id: Option<String>) -> Result<Self, AiError> {
        let url = url::Url::parse(&redirect).map_err(|_| invalid())?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || url.path() != "/auth/callback"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        if client_id.as_ref().is_some_and(|id| !issued_client(id)) {
            return Err(invalid());
        }
        Ok(Self {
            verifier: random()?,
            state: random()?,
            nonce: random()?,
            redirect,
            client_id: client_id.unwrap_or_else(|| "dynamic_agent_client".into()),
        })
    }

    pub fn authorization_url(&self, protocol: &Protocol, host: &str) -> Result<String, AiError> {
        if !host.starts_with("urn:uuid:") || uuid::Uuid::parse_str(&host[9..]).is_err() {
            return Err(invalid());
        }
        let mut url = url::Url::parse(&protocol.authorization).map_err(|_| invalid())?;
        let challenge =
            URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, self.verifier.as_bytes()));
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect.as_str()),
            (
                "scope",
                &format!("openid profile email offline_access resource.invoke {PLAN_SCOPE}"),
            ),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", self.state.as_str()),
            ("nonce", self.nonce.as_str()),
            ("resource", RESOURCE),
            ("ext_agent_host_id", host),
        ]);
        if self.client_id == "dynamic_agent_client" {
            url.query_pairs_mut()
                .append_pair("agent_name_hint", "Qiyas");
        }
        Ok(url.into())
    }

    /// Duplicate state/code parameters, wrong paths and callbacks without state are refused.
    pub fn callback_code(&self, target: &str) -> Result<Grant, AiError> {
        let expected = url::Url::parse(&self.redirect).map_err(|_| invalid())?;
        let callback =
            url::Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| invalid())?;
        if callback.path() != expected.path() || callback.fragment().is_some() {
            return Err(invalid());
        }
        let pairs: Vec<_> = callback.query_pairs().collect();
        let one = |name: &str| -> Result<String, AiError> {
            let values: Vec<_> = pairs
                .iter()
                .filter(|(k, _)| k == name)
                .map(|(_, v)| v.to_string())
                .collect();
            if values.len() == 1 && !values[0].is_empty() {
                Ok(values[0].clone())
            } else {
                Err(invalid())
            }
        };
        let state = one("state")?;
        if state != self.state {
            return Err(invalid());
        }
        // This is the code flow. Never accept token-bearing or mixed success/error callbacks.
        if pairs
            .iter()
            .any(|(key, _)| matches!(key.as_ref(), "access_token" | "refresh_token" | "id_token"))
            || (pairs.iter().any(|(key, _)| key == "error")
                && pairs.iter().any(|(key, _)| key == "code"))
        {
            return Err(invalid());
        }
        if pairs.iter().any(|(key, _)| key == "error") {
            return Err(if one("error")? == "access_denied" {
                AiError::Subscription("chatgpt_scope")
            } else {
                invalid()
            });
        }
        let client_id = if pairs.iter().any(|(key, _)| key == "client_id") {
            let issued = one("client_id")?;
            if !issued_client(&issued)
                || (self.client_id != "dynamic_agent_client" && issued != self.client_id)
            {
                return Err(invalid());
            }
            issued
        } else if self.client_id != "dynamic_agent_client" {
            self.client_id.clone()
        } else {
            return Err(invalid());
        };
        Ok(Grant {
            code: one("code")?,
            client_id,
        })
    }
}

/// Wait for exactly this sign-in's browser callback on an already-bound loopback socket.
/// URLs, authorization codes and HTTP headers never enter a log or the response page.
pub async fn loopback_code(
    listener: tokio::net::TcpListener,
    pending: &Pending,
    cancel: &CancellationToken,
) -> Result<Grant, AiError> {
    let receive = async {
        for _ in 0..32 {
            let (mut socket, peer) = listener.accept().await.map_err(|_| invalid())?;
            if !peer.ip().is_loopback() {
                continue;
            }
            let read = async {
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let size = socket.read(&mut chunk).await.map_err(|_| invalid())?;
                    if size == 0 || request.len() + size > 16 * 1024 {
                        return Err(invalid());
                    }
                    request.extend_from_slice(&chunk[..size]);
                    if request.windows(4).any(|s| s == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = std::str::from_utf8(&request).map_err(|_| invalid())?;
                let mut parts = text.lines().next().ok_or_else(invalid)?.split_whitespace();
                if parts.next() != Some("GET") {
                    return Err(invalid());
                }
                let target = parts.next().ok_or_else(invalid)?;
                if !target.starts_with('/') || target.starts_with("//") {
                    return Err(invalid());
                }
                pending.callback_code(target)
            };
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), read).await;
            let (status, body) = match &result {
                Ok(Ok(_)) => ("200 OK", "Sign-in received. You can return to Qiyas."),
                _ => (
                    "400 Bad Request",
                    "This sign-in could not be verified. Return to Qiyas.",
                ),
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                socket.write_all(response.as_bytes()),
            )
            .await;
            if let Ok(Ok(code)) = result {
                return Ok(code);
            }
            if let Ok(Err(error @ AiError::Subscription("chatgpt_scope"))) = result {
                return Err(error);
            }
        }
        Err(invalid())
    };
    tokio::select! {
        result = tokio::time::timeout(std::time::Duration::from_secs(300), receive) =>
            result.map_err(|_| AiError::Subscription("chatgpt_cancelled"))?,
        () = cancel.cancelled() => Err(AiError::Subscription("chatgpt_cancelled")),
    }
}

impl Protocol {
    pub async fn exchange(&self, pending: Pending, grant: Grant) -> Result<Tokens, AiError> {
        if !issued_client(&grant.client_id)
            || (pending.client_id != "dynamic_agent_client" && grant.client_id != pending.client_id)
        {
            return Err(invalid());
        }
        let fields = [
            ("grant_type", "authorization_code"),
            ("code", grant.code.as_str()),
            ("client_id", grant.client_id.as_str()),
            ("redirect_uri", pending.redirect.as_str()),
            ("code_verifier", pending.verifier.as_str()),
            ("resource", RESOURCE),
        ];
        let response = self.token_request(&fields).await?;
        let jwks = self.keys().await?;
        tokens_from_response(
            response,
            &jwks,
            self,
            &grant.client_id,
            Some(&pending.nonce),
            None,
        )
    }

    pub async fn refresh(&self, old: Tokens) -> Result<Tokens, AiError> {
        let refresh = old
            .refresh_token
            .as_deref()
            .ok_or(AiError::Subscription("chatgpt_auth"))?;
        // Fetch verification keys before rotating a refresh token. A discovery outage
        // must not discard an already-issued replacement and force reuse of the old token.
        let jwks = self.keys().await?;
        let response = self
            .token_request(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
                ("client_id", &old.client_id),
                ("resource", RESOURCE),
            ])
            .await?;
        tokens_from_response(response, &jwks, self, &old.client_id, None, Some(&old))
    }

    async fn token_request(&self, fields: &[(&str, &str)]) -> Result<Value, AiError> {
        let encoded = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields.iter().copied())
            .finish();
        let response = crate::provider::http_client()
            .post(&self.token)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(encoded)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| AiError::Subscription("chatgpt_temporary"))?;
        if response.status().is_server_error() {
            return Err(AiError::Subscription("chatgpt_temporary"));
        }
        if !response.status().is_success() {
            let body: Value = response.json().await.map_err(|_| invalid())?;
            let code = body["error"]
                .as_str()
                .or_else(|| body["error"]["code"].as_str())
                .unwrap_or("");
            return Err(AiError::Subscription(match code {
                "invalid_client" => "chatgpt_setup",
                "invalid_grant"
                | "invalid_refresh_token"
                | "token_expired"
                | "refresh_token_expired"
                | "refresh_token_invalidated"
                | "refresh_token_reused" => "chatgpt_reauthorize",
                "access_denied" => "chatgpt_scope",
                _ => "chatgpt_oauth",
            }));
        }
        response.json().await.map_err(|_| invalid())
    }

    async fn discovery_document(&self) -> Result<Value, AiError> {
        let response = crate::provider::http_client()
            .get(&self.discovery)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|_| invalid())?;
        if !response.status().is_success() {
            return Err(invalid());
        }
        let doc: Value = response.json().await.map_err(|_| invalid())?;
        if doc["issuer"] != self.issuer {
            return Err(invalid());
        }
        Ok(doc)
    }

    // Discovery is trusted only at the pinned issuer. Never follow redirects or send
    // credentials to HTTP destinations outside local mock servers.
    fn discovered_url(&self, doc: &Value, key: &str) -> Result<String, AiError> {
        let value = doc[key].as_str().ok_or_else(invalid)?;
        let url = url::Url::parse(value).map_err(|_| invalid())?;
        let discovery = url::Url::parse(&self.discovery).map_err(|_| invalid())?;
        let local_mock = discovery.scheme() == "http"
            && discovery.host_str() == Some("127.0.0.1")
            && url.origin() == discovery.origin();
        if (!local_mock && url.scheme() != "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        Ok(value.into())
    }

    async fn keys(&self) -> Result<Value, AiError> {
        let doc = self.discovery_document().await?;
        let response = crate::provider::http_client()
            .get(self.discovered_url(&doc, "jwks_uri")?)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|_| invalid())?;
        if !response.status().is_success() {
            return Err(invalid());
        }
        response.json().await.map_err(|_| invalid())
    }

    /// Clear local tokens even when the provider cannot confirm revocation.
    pub async fn revoke(&self, tokens: &Tokens) -> bool {
        for attempt in 0..3 {
            let result = async {
                let doc = self.discovery_document().await?;
                let url = self.discovered_url(&doc, "revocation_endpoint")?;
                let refresh = tokens.refresh_token.as_deref().ok_or_else(invalid)?;
                let encoded = url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs([
                        ("token", refresh),
                        ("token_type_hint", "refresh_token"),
                        ("client_id", &tokens.client_id),
                    ])
                    .finish();
                let response = crate::provider::http_client()
                    .post(url)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(encoded)
                    .timeout(std::time::Duration::from_secs(15))
                    .send()
                    .await;
                match response {
                    Ok(r) if r.status().as_u16() == 200 => Ok(true),
                    Ok(r) if !r.status().is_server_error() => Ok(false),
                    _ => Err(invalid()),
                }
            }
            .await;
            if let Ok(confirmed) = result {
                return confirmed;
            }
            if attempt < 2 {
                tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
            }
        }
        false
    }
}

fn tokens_from_response(
    value: Value,
    jwks: &Value,
    protocol: &Protocol,
    client: &str,
    nonce: Option<&str>,
    old: Option<&Tokens>,
) -> Result<Tokens, AiError> {
    if !value["token_type"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("bearer"))
    {
        return Err(invalid());
    }
    let access = value["access_token"]
        .as_str()
        .filter(|s| !s.is_empty() && s.is_ascii() && !s.chars().any(char::is_whitespace))
        .ok_or_else(invalid)?;
    let expires_in = value["expires_in"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(invalid)?;
    let scopes = match value["scope"].as_str() {
        Some(scope) => scope.split_whitespace().map(str::to_owned).collect(),
        None => Vec::new(),
    };
    let (id_token, claims) = if let Some(id) = value["id_token"].as_str() {
        let claims = validate_id_token(
            id,
            jwks,
            &protocol.issuer,
            client,
            nonce,
            Some(access),
            now(),
        )?;
        if old.is_some_and(|old| old.subject != claims["sub"].as_str().unwrap_or("")) {
            return Err(invalid());
        }
        (id.to_owned(), claims)
    } else if let Some(old) = old {
        // OIDC refresh may omit the ID token; retain the identity verified at sign-in.
        (
            old.id_token.clone(),
            json!({"sub":old.subject,"name":old.display_name}),
        )
    } else {
        return Err(invalid());
    };
    let tokens = Tokens {
        access_token: access.into(),
        id_token,
        scopes,
        refresh_token: value["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        expires_at: now().checked_add(expires_in).ok_or_else(invalid)?,
        client_id: client.into(),
        subject: claims["sub"].as_str().ok_or_else(invalid)?.into(),
        display_name: claims["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("ChatGPT")
            .chars()
            .take(160)
            .collect(),
    };
    if tokens.refresh_token.is_none() {
        return Err(invalid());
    }
    Ok(tokens)
}

/// Verify the signature before trusting any claim. Only RS256 signing keys from the pinned
/// issuer's JWKS are accepted; unsigned tokens, key URLs and algorithm substitution fail.
pub fn validate_id_token(
    token: &str,
    jwks: &Value,
    issuer: &str,
    audience: &str,
    nonce: Option<&str>,
    access: Option<&str>,
    timestamp: u64,
) -> Result<Value, AiError> {
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3 || token.len() > 64 * 1024 {
        return Err(invalid());
    }
    let decode = |value: &str| URL_SAFE_NO_PAD.decode(value).map_err(|_| invalid());
    let header: Value = serde_json::from_slice(&decode(parts[0])?).map_err(|_| invalid())?;
    if header["alg"] != "RS256" || header.get("crit").is_some() {
        return Err(invalid());
    }
    let kid = header["kid"].as_str().ok_or_else(invalid)?;
    let keys: Vec<_> = jwks["keys"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .filter(|k| {
            k["kid"] == kid
                && k["kty"] == "RSA"
                && k.get("use").is_none_or(|v| v == "sig")
                && k.get("alg").is_none_or(|v| v == "RS256")
        })
        .collect();
    if keys.len() != 1 {
        return Err(invalid());
    }
    let key = keys[0];
    let n = decode(key["n"].as_str().ok_or_else(invalid)?)?;
    let e = decode(key["e"].as_str().ok_or_else(invalid)?)?;
    signature::RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &signature::RSA_PKCS1_2048_8192_SHA256,
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &decode(parts[2])?,
        )
        .map_err(|_| invalid())?;
    let claims: Value = serde_json::from_slice(&decode(parts[1])?).map_err(|_| invalid())?;
    let audiences: Vec<_> = match &claims["aud"] {
        Value::String(s) => vec![s.as_str()],
        Value::Array(a) if a.iter().all(Value::is_string) => {
            a.iter().filter_map(Value::as_str).collect()
        }
        _ => return Err(invalid()),
    };
    if claims["iss"] != issuer
        || !audiences.contains(&audience)
        || (audiences.len() > 1 && claims["azp"] != audience)
        || claims.get("azp").is_some_and(|azp| azp != audience)
        || claims["exp"].as_u64().is_none_or(|exp| exp <= timestamp)
        || claims["iat"]
            .as_u64()
            .is_none_or(|iat| iat > timestamp.saturating_add(60))
        || claims.get("nbf").is_some_and(|nbf| {
            nbf.as_u64()
                .is_none_or(|n| n > timestamp.saturating_add(60))
        })
        || claims["sub"].as_str().is_none_or(str::is_empty)
        || nonce.is_some_and(|nonce| claims["nonce"] != nonce)
    {
        return Err(invalid());
    }
    if let Some(hash) = claims.get("at_hash") {
        let access = access.ok_or_else(invalid)?;
        let digest = digest::digest(&digest::SHA256, access.as_bytes());
        if hash != &Value::String(URL_SAFE_NO_PAD.encode(&digest.as_ref()[..16])) {
            return Err(invalid());
        }
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;

    pub(super) fn signed(claims: &Value) -> String {
        let key = signature::RsaKeyPair::from_pkcs8(include_bytes!(
            "../../tests/fixtures/fake-oidc-private.der"
        ))
        .unwrap();
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"qiyas-test-only"}"#);
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        let message = format!("{header}.{payload}");
        let mut signature = vec![0; key.public_key().modulus_len()];
        key.sign(
            &signature::RSA_PKCS1_SHA256,
            &rand::SystemRandom::new(),
            message.as_bytes(),
            &mut signature,
        )
        .unwrap();
        format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature))
    }
    fn keys() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/fake-oidc-jwks.json")).unwrap()
    }
    fn protocol(base: &str) -> Protocol {
        Protocol {
            authorization: format!("{base}/authorize"),
            token: format!("{base}/token"),
            issuer: "https://test-issuer.invalid".into(),
            discovery: format!("{base}/discovery"),
            responses: format!("{base}/responses"),
            models: format!("{base}/models"),
        }
    }
    fn claims() -> Value {
        json!({"iss":"https://test-issuer.invalid","aud":"dynamic_agent_client","sub":"test-subject",
            "name":"Test Surveyor","nonce":"test-nonce","iat":1000,"exp":2000})
    }

    #[test]
    fn validates_signature_issuer_audience_nonce_expiry_and_access_hash() {
        let valid = claims();
        let verify = |value: &Value| {
            validate_id_token(
                &signed(value),
                &keys(),
                "https://test-issuer.invalid",
                "dynamic_agent_client",
                Some("test-nonce"),
                Some("fake-access-token"),
                1100,
            )
        };
        assert_eq!(verify(&valid).unwrap()["name"], "Test Surveyor");
        for (field, bad) in [
            ("iss", json!("https://attacker.invalid")),
            ("aud", json!("other-client")),
            ("nonce", json!("wrong")),
            ("exp", json!(1099)),
            ("iat", json!(9999)),
            ("sub", json!("")),
            ("azp", json!("another-client")),
            ("nbf", json!(9999)),
            ("at_hash", json!("wrong")),
        ] {
            let mut changed = valid.clone();
            changed[field] = bad;
            assert!(verify(&changed).is_err(), "{field}");
        }
        let mut multi = valid.clone();
        multi["aud"] = json!(["dynamic_agent_client", "other"]);
        assert!(verify(&multi).is_err());
        multi["azp"] = json!("dynamic_agent_client");
        assert!(verify(&multi).is_ok());
        let token = signed(&valid);
        let mut tampered = token.into_bytes();
        let end = tampered.len() - 10;
        tampered[end] = if tampered[end] == b'A' { b'B' } else { b'A' };
        assert!(validate_id_token(
            std::str::from_utf8(&tampered).unwrap(),
            &keys(),
            "https://test-issuer.invalid",
            "dynamic_agent_client",
            Some("test-nonce"),
            None,
            1100
        )
        .is_err());
        assert!(validate_id_token(
            "eyJhbGciOiJub25lIn0.e30.",
            &keys(),
            "",
            "",
            None,
            None,
            1100
        )
        .is_err());
    }

    #[test]
    fn pkce_state_nonce_and_callback_are_bound_to_one_attempt() {
        let pending = Pending::new("http://127.0.0.1:43210/auth/callback".into(), None).unwrap();
        let p = protocol("https://test-issuer.invalid");
        let url = url::Url::parse(
            &pending
                .authorization_url(&p, "urn:uuid:814178b2-ff06-4415-98ed-41518f0f88a8")
                .unwrap(),
        )
        .unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["client_id"], "dynamic_agent_client");
        assert_eq!(params["agent_name_hint"], "Qiyas");
        assert_eq!(
            params["ext_agent_host_id"],
            "urn:uuid:814178b2-ff06-4415-98ed-41518f0f88a8"
        );
        assert!(params["scope"]
            .split_whitespace()
            .any(|scope| scope == PLAN_SCOPE));
        assert_eq!(params["code_challenge_method"], "S256");
        assert_ne!(params["code_challenge"], pending.verifier);
        assert_eq!(
            params["code_challenge"],
            URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, pending.verifier.as_bytes()))
        );
        assert_ne!(pending.state, pending.nonce);
        assert_eq!(
            pending
                .callback_code(&format!(
                    "/auth/callback?state={}&code=test-code&client_id=oaiapp_test",
                    pending.state
                ))
                .unwrap()
                .code,
            "test-code"
        );
        for target in [
            "/auth/callback?state=wrong&code=test",
            "/auth/callback?code=test",
            "/wrong?code=test",
            &format!(
                "/auth/callback?state={}&state={}&code=test",
                pending.state, pending.state
            ),
            &format!("/auth/callback?state={}&code=a&code=b", pending.state),
            &format!(
                "/auth/callback?state={}&code=test&error=test",
                pending.state
            ),
            &format!(
                "/auth/callback?state={}&access_token=fake&error_description=fake&error=test",
                pending.state
            ),
        ] {
            assert!(pending.callback_code(target).is_err(), "{target}");
        }
    }

    #[test]
    fn missing_scope_keeps_identity_but_blocks_plan_and_refresh_requires_rotated_tokens() {
        let p = protocol("https://test-issuer.invalid");
        let mut claims = claims();
        claims["iat"] = json!(now());
        claims["exp"] = json!(now() + 3600);
        let body = json!({"token_type":"Bearer","access_token":"fake-access-token","refresh_token":"fake-refresh-token",
            "expires_in":3600,"id_token":signed(&claims),"scope":format!("openid profile {PLAN_SCOPE}")});
        let tokens = tokens_from_response(
            body.clone(),
            &keys(),
            &p,
            "dynamic_agent_client",
            Some("test-nonce"),
            None,
        )
        .unwrap();
        assert_eq!(tokens.display_name, "Test Surveyor");
        for scope in [
            Value::Null,
            json!("openid profile"),
            json!("chatgpt.tokens.use.direct.extra"),
        ] {
            let mut body = body.clone();
            body["scope"] = scope;
            assert!(tokens_from_response(
                body,
                &keys(),
                &p,
                "dynamic_agent_client",
                Some("test-nonce"),
                None
            )
            .unwrap()
            .require_plan()
            .is_err());
        }
        let refreshed = tokens_from_response(
            json!({"token_type":"Bearer","access_token":"fake-new-access","refresh_token":"fake-rotated","expires_in":3600,"scope":"openid"}),
            &keys(),
            &p,
            "dynamic_agent_client",
            None,
            Some(&tokens),
        )
        .unwrap();
        assert_eq!(refreshed.refresh_token.as_deref(), Some("fake-rotated"));
        assert_eq!(refreshed.scopes, vec!["openid"]);
        assert!(refreshed.require_plan().is_err());
        claims["sub"] = json!("another-person");
        let mut changed = body;
        changed["id_token"] = json!(signed(&claims));
        assert!(tokens_from_response(
            changed,
            &keys(),
            &p,
            "dynamic_agent_client",
            None,
            Some(&tokens)
        )
        .is_err());
    }

    #[tokio::test]
    #[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
    async fn mock_token_and_jwks_servers_exchange_without_real_accounts() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock OAuth needs a loopback port");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let p = protocol(&base);
        let pending = Pending::new("http://127.0.0.1:43210/auth/callback".into(), None).unwrap();
        let grant = pending
            .callback_code(&format!(
                "/auth/callback?state={}&code=fake-code&client_id=oaiapp_test",
                pending.state
            ))
            .unwrap();
        let nonce = pending.nonce.clone();
        let mut claims = claims();
        claims["nonce"] = json!(nonce);
        claims["aud"] = json!("oaiapp_test");
        claims["iat"] = json!(now());
        claims["exp"] = json!(now() + 3600);
        let response = json!({"token_type":"Bearer","access_token":"fake-access-token","refresh_token":"fake-refresh-token",
            "id_token":signed(&claims),"expires_in":3600,"scope":format!("openid {PLAN_SCOPE}")});
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for body in [
                response,
                json!({"issuer":"https://test-issuer.invalid","jwks_uri":format!("{base}/jwks"),"revocation_endpoint":format!("{base}/revoke")}),
                keys(),
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 16384];
                let len = socket.read(&mut buf).await.unwrap();
                requests.push(String::from_utf8(buf[..len].to_vec()).unwrap());
                let body = body.to_string();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
            }
            requests
        });
        let tokens = p.exchange(pending, grant).await.unwrap();
        tokens.require_plan().unwrap();
        let requests = server.await.unwrap();
        assert!(requests[0].contains("code_verifier="));
        assert!(requests[0].contains("grant_type=authorization_code"));
        assert!(requests[0].contains("client_id=oaiapp_test"));
        assert!(requests[0].contains("resource=https%3A%2F%2Fapi.openai.com%2Fv1"));
        assert!(requests[1].starts_with("GET /discovery "));
        assert!(requests[2].starts_with("GET /jwks "));
        assert!(!requests[1].contains("fake-access-token"));
    }

    #[tokio::test]
    #[ignore = "Requires a socket-capable runner; reviewer runs mock servers"]
    async fn mock_loopback_ignores_wrong_state_and_never_echoes_the_code() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock OAuth callback needs a loopback port");
        let addr = listener.local_addr().unwrap();
        let pending = Pending::new(
            format!("http://127.0.0.1:{}/auth/callback", addr.port()),
            None,
        )
        .unwrap();
        let state = pending.state.clone();
        let receive = tokio::spawn(async move {
            loopback_code(listener, &pending, &CancellationToken::new()).await
        });
        for (query, status) in [
            ("state=wrong&code=fake-code".to_string(), "400 Bad Request"),
            (
                format!("state={state}&code=fake-code&client_id=oaiapp_test"),
                "200 OK",
            ),
        ] {
            let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
            socket
                .write_all(
                    format!("GET /auth/callback?{query} HTTP/1.1\r\nHost: {addr}\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            let mut answer = String::new();
            socket.read_to_string(&mut answer).await.unwrap();
            assert!(answer.starts_with(&format!("HTTP/1.1 {status}")));
            assert!(!answer.contains("fake-code"));
            assert!(!answer.contains(&state));
        }
        assert_eq!(receive.await.unwrap().unwrap().code, "fake-code");
    }

    #[test]
    fn issued_client_is_bound_and_consent_errors_require_state() {
        let p = protocol("https://test-issuer.invalid");
        let pending = Pending::new(
            "http://127.0.0.1:43123/auth/callback".into(),
            Some("oaiapp_test".into()),
        )
        .unwrap();
        let url = url::Url::parse(
            &pending
                .authorization_url(&p, "urn:uuid:814178b2-ff06-4415-98ed-41518f0f88a8")
                .unwrap(),
        )
        .unwrap();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert!(!pairs.contains_key("agent_name_hint"));
        assert!(!pairs.contains_key("id_token_hint"));
        assert_eq!(pairs["resource"], RESOURCE);
        assert_eq!(
            pairs["scope"],
            "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct"
        );
        assert_eq!(
            pending
                .callback_code(&format!("/auth/callback?state={}&code=fake", pending.state))
                .unwrap()
                .client_id,
            "oaiapp_test"
        );
        assert!(pending
            .callback_code(&format!(
                "/auth/callback?state={}&code=fake&client_id=oaiapp_other",
                pending.state
            ))
            .is_err());
        assert_eq!(
            pending
                .callback_code(&format!(
                    "/auth/callback?state={}&error=access_denied",
                    pending.state
                ))
                .err()
                .unwrap()
                .code(),
            "chatgpt_scope"
        );
        assert_eq!(
            pending
                .callback_code("/auth/callback?state=wrong&error=access_denied")
                .err()
                .unwrap()
                .code(),
            "chatgpt_oauth"
        );
        assert!(Pending::new("http://localhost:43123/auth/callback".into(), None).is_err());
        assert!(Pending::new("http://127.0.0.1:43123/wrong".into(), None).is_err());
        assert!(Pending::new(
            "http://127.0.0.1:43123/auth/callback".into(),
            Some("someone@example.test".into())
        )
        .is_err());
        let fresh = Pending::new("http://127.0.0.1:43123/auth/callback".into(), None).unwrap();
        assert_ne!(fresh.state, pending.state);
        assert_ne!(fresh.nonce, pending.nonce);
        assert!(fresh
            .callback_code(&format!("/auth/callback?state={}&code=fake", fresh.state))
            .is_err());
    }
}
