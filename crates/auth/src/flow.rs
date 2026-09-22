//! The authorization code flow, with PKCE.
//!
//! Code flow rather than implicit, and PKCE even though this server can
//! keep a secret: PKCE binds the code to the browser that asked for it, so
//! a code intercepted in a redirect — a referrer header, shoulder-surfing,
//! a badly written proxy — is worth nothing without the verifier that
//! never left this process.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use otelview_config::OidcConfig;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::provider::Provider;
use crate::session::{constant_time_eq, random_id};

/// A login in progress: issued at the redirect to the provider, spent
/// when the browser comes back.
#[derive(Debug, Clone)]
pub struct PendingLogin {
    pub verifier: String,
    pub nonce: String,
    /// Where to send the browser once it is signed in.
    pub return_to: String,
    started: Instant,
}

/// How long a login may take before its state is thrown away. Long enough
/// to read a password manager, type a TOTP code and tap a passkey; short
/// enough that abandoned attempts do not pile up.
const LOGIN_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Cap on logins in flight, so an open `/auth/login` cannot be used to
/// grow this table without bound.
const MAX_PENDING: usize = 1_000;

/// The logins currently in flight, keyed by the `state` parameter.
#[derive(Default)]
pub struct PendingLogins(Mutex<HashMap<String, PendingLogin>>);

impl PendingLogins {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a login and return its `state`.
    pub fn start(&self, verifier: String, nonce: String, return_to: String) -> String {
        let state = random_id();
        let mut pending = self.0.lock().unwrap_or_else(|e| e.into_inner());
        pending.retain(|_, p| p.started.elapsed() < LOGIN_WINDOW);
        if pending.len() >= MAX_PENDING {
            tracing::warn!("too many logins in flight; dropping the oldest");
            pending.clear();
        }
        pending.insert(
            state.clone(),
            PendingLogin {
                verifier,
                nonce,
                return_to,
                started: Instant::now(),
            },
        );
        state
    }

    /// Spend a `state`. It works once: a replayed callback finds nothing,
    /// which is what makes `state` a defence against CSRF rather than
    /// decoration.
    pub fn take(&self, state: &str) -> Option<PendingLogin> {
        let mut pending = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // Constant-time lookup is not meaningful for a HashMap, but the
        // window check is: an expired login must not be usable.
        let found = pending.remove(state)?;
        (found.started.elapsed() < LOGIN_WINDOW).then_some(found)
    }

    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A PKCE verifier and the challenge derived from it.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        let verifier = random_id();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// Percent-encode a query-string value.
///
/// Hand-rolled to keep a URL crate out of the dependency list for one
/// function: everything outside the unreserved set of RFC 3986 is escaped,
/// which is stricter than necessary and never wrong.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The URL to send the browser to, to sign in.
pub async fn authorization_url(
    cfg: &OidcConfig,
    provider: &Provider,
    pkce: &Pkce,
    state: &str,
    nonce: &str,
) -> Result<String> {
    let metadata = provider.metadata().await?;
    if !metadata.supports_pkce_s256() {
        bail!(
            "the provider at {} does not advertise PKCE with S256, which this server requires",
            metadata.issuer
        );
    }
    let scopes = if cfg.scopes.is_empty() {
        "openid".to_string()
    } else {
        cfg.scopes.join(" ")
    };
    let separator = if metadata.authorization_endpoint.contains('?') {
        '&'
    } else {
        '?'
    };
    Ok(format!(
        "{}{separator}response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}\
         &code_challenge={}&code_challenge_method=S256",
        metadata.authorization_endpoint,
        encode(&cfg.client_id),
        encode(&cfg.redirect_url),
        encode(&scopes),
        encode(state),
        encode(nonce),
        encode(&pkce.challenge),
    ))
}

/// What the provider returns when a code is exchanged.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
}

/// Trade an authorization code for tokens.
pub async fn exchange_code(
    cfg: &OidcConfig,
    provider: &Provider,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse> {
    let metadata = provider.metadata().await?;
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", cfg.redirect_url.clone()),
        ("client_id", cfg.client_id.clone()),
        ("code_verifier", verifier.to_string()),
    ];
    // A confidential client authenticates as well as proving the
    // verifier; a public one relies on PKCE alone, which is what it is for.
    let secret = cfg.resolved_client_secret();
    if let Some(secret) = &secret {
        form.push(("client_secret", secret.clone()));
    }

    let resp = provider
        .http()
        .post(&metadata.token_endpoint)
        .form(&form)
        .send()
        .await
        .with_context(|| format!("POST {}", metadata.token_endpoint))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // The provider's error body is the useful part — "invalid_grant"
        // versus "invalid_client" is the difference between a stale login
        // and a misconfigured secret.
        bail!(
            "the provider refused the authorization code ({status}): {}",
            body.trim()
        );
    }
    serde_json::from_str(&body).context("parsing the token response")
}

/// The provider's end-session URL, when it publishes one.
pub async fn logout_url(
    cfg: &OidcConfig,
    provider: &Provider,
    id_token: Option<&str>,
) -> Result<Option<String>> {
    let metadata = provider.metadata().await?;
    let Some(endpoint) = metadata.end_session_endpoint.clone() else {
        return Ok(None);
    };
    let mut url = format!("{endpoint}?client_id={}", encode(&cfg.client_id));
    if let Some(token) = id_token {
        url.push_str(&format!("&id_token_hint={}", encode(token)));
    }
    if let Some(post) = &cfg.post_logout_redirect_url {
        url.push_str(&format!("&post_logout_redirect_uri={}", encode(post)));
    }
    Ok(Some(url))
}

/// Check the `nonce` in an id token against the one this server sent.
///
/// The nonce binds the id token to this login attempt; without the check
/// a token replayed from elsewhere would be accepted.
pub fn nonce_matches(id_token_claims: &serde_json::Value, expected: &str) -> bool {
    id_token_claims
        .get("nonce")
        .and_then(|v| v.as_str())
        .is_some_and(|got| constant_time_eq(got.as_bytes(), expected.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pkce_challenge_is_the_sha256_of_its_verifier() {
        let pkce = Pkce::new();
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier.as_bytes()));
        assert_eq!(pkce.challenge, expected);
        // Base64url, so it survives a query string unescaped.
        assert!(!pkce.challenge.contains('+') && !pkce.challenge.contains('/'));
        assert!(!pkce.challenge.contains('='));
    }

    /// The RFC 7636 worked example, so this is checked against the spec
    /// rather than against itself.
    #[test]
    fn the_rfc_example_verifier_produces_the_rfc_challenge() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn two_pkce_pairs_differ() {
        assert_ne!(Pkce::new().verifier, Pkce::new().verifier);
    }

    #[test]
    fn a_state_can_only_be_spent_once() {
        let pending = PendingLogins::new();
        let state = pending.start("v".into(), "n".into(), "/".into());
        assert!(pending.take(&state).is_some());
        // A replayed callback finds nothing.
        assert!(pending.take(&state).is_none());
        assert!(pending.is_empty());
    }

    #[test]
    fn an_unknown_state_is_not_accepted() {
        let pending = PendingLogins::new();
        pending.start("v".into(), "n".into(), "/".into());
        assert!(pending.take("not-a-state").is_none());
    }

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(encode("openid profile"), "openid%20profile");
        assert_eq!(
            encode("https://otelview.example.com/auth/callback"),
            "https%3A%2F%2Fotelview.example.com%2Fauth%2Fcallback"
        );
        // Zitadel's audience scope is full of colons.
        assert_eq!(
            encode("urn:zitadel:iam:org:project:id:123:aud"),
            "urn%3Azitadel%3Aiam%3Aorg%3Aproject%3Aid%3A123%3Aaud"
        );
        assert_eq!(encode("safe-_.~"), "safe-_.~");
    }

    #[test]
    fn a_nonce_must_match_exactly() {
        let claims = serde_json::json!({"nonce": "abc123"});
        assert!(nonce_matches(&claims, "abc123"));
        assert!(!nonce_matches(&claims, "abc124"));
        assert!(!nonce_matches(&claims, "abc"));
        assert!(!nonce_matches(&serde_json::json!({}), "abc123"));
    }

    #[test]
    fn a_token_response_parses_past_extra_fields() {
        let body = serde_json::json!({
            "access_token": "at",
            "token_type": "Bearer",
            "expires_in": 3600,
            "id_token": "it",
            "scope": "openid profile",
            "something_else": 1
        });
        let parsed: TokenResponse = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.access_token, "at");
        assert_eq!(parsed.id_token.as_deref(), Some("it"));
        assert!(parsed.refresh_token.is_none());
    }
}
