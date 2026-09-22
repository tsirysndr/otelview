//! Deciding whether an access token is genuine.
//!
//! Two ways, because providers issue two kinds of token. A JWT carries its
//! own claims and is checked here against the provider's published signing
//! keys — no network call on the request path. An opaque token is a
//! reference the provider has to resolve, which means asking it (RFC 7662)
//! on every request. Zitadel issues either, depending on how the app is
//! configured; JWTs are preferred and the code says so.

use std::slice;
use std::time::Duration;

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use otelview_config::OidcConfig;
use serde_json::Value;

use crate::provider::Provider;

/// Why a token was not accepted.
///
/// The split matters at the edge: `Invalid` is a 401 the caller can fix by
/// logging in again, while `Unavailable` means this server could not
/// reach the provider — a 503, and not the caller's fault at all.
#[derive(Debug)]
pub enum TokenError {
    Invalid(String),
    Unavailable(anyhow::Error),
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenError::Invalid(msg) => write!(f, "{msg}"),
            TokenError::Unavailable(e) => write!(f, "{e:#}"),
        }
    }
}

/// Signature algorithms accepted from the provider.
///
/// An allowlist, not whatever the token's header asks for: `alg: none` and
/// algorithm-confusion attacks both begin with trusting that field.
const ALLOWED_ALGORITHMS: &[Algorithm] = &[
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
];

/// Verify an access token and return its claims.
pub async fn verify(
    cfg: &OidcConfig,
    provider: &Provider,
    leeway: Duration,
    token: &str,
) -> Result<Value, TokenError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(TokenError::Invalid("the token is empty".into()));
    }
    // Three dot-separated parts is what a JWS looks like; anything else is
    // a reference the provider has to resolve for us.
    if token.split('.').count() == 3 {
        verify_jwt(cfg, provider, leeway, token).await
    } else if cfg.introspection {
        introspect(cfg, provider, token).await
    } else {
        Err(TokenError::Invalid(
            "this access token is opaque, and auth.oidc.introspection is off. Request an \
             audience scope so the provider issues a JWT, or turn introspection on."
                .into(),
        ))
    }
}

/// Verify the ID token that came back with an access token.
///
/// Different from an access token in two ways that matter: it is
/// addressed to this client rather than to the API's audience, and it
/// carries the `nonce` from the authorization request. Checking that
/// nonce is what stops an ID token captured elsewhere being replayed
/// into this login.
///
/// It is also where the provider puts who the user *is* — `name`,
/// `email` — which an access token is not obliged to carry and which
/// Zitadel does not.
pub async fn verify_id_token(
    cfg: &OidcConfig,
    provider: &Provider,
    leeway: Duration,
    token: &str,
    expected_nonce: &str,
) -> Result<Value, TokenError> {
    // An id token is addressed to this client, not to the API audience.
    let claims = verify_with_audiences(
        cfg,
        provider,
        leeway,
        token,
        slice::from_ref(&cfg.client_id),
    )
    .await?;
    if !crate::flow::nonce_matches(&claims, expected_nonce) {
        return Err(TokenError::Invalid(
            "the id token's nonce does not match this login attempt".into(),
        ));
    }
    Ok(claims)
}

async fn verify_jwt(
    cfg: &OidcConfig,
    provider: &Provider,
    leeway: Duration,
    token: &str,
) -> Result<Value, TokenError> {
    verify_with_audiences(cfg, provider, leeway, token, &cfg.accepted_audiences()).await
}

async fn verify_with_audiences(
    cfg: &OidcConfig,
    provider: &Provider,
    leeway: Duration,
    token: &str,
    audiences: &[String],
) -> Result<Value, TokenError> {
    let header = decode_header(token)
        .map_err(|e| TokenError::Invalid(format!("this is not a readable JWT: {e}")))?;
    if !ALLOWED_ALGORITHMS.contains(&header.alg) {
        return Err(TokenError::Invalid(format!(
            "token signed with {:?}, which this server does not accept",
            header.alg
        )));
    }
    let kid = header
        .kid
        .ok_or_else(|| TokenError::Invalid("the token names no signing key (kid)".into()))?;

    let jwk = provider
        .signing_key(&kid)
        .await
        .map_err(|e| TokenError::Invalid(format!("{e:#}")))?;
    let key = DecodingKey::from_jwk(&jwk)
        .map_err(|e| TokenError::Unavailable(anyhow::anyhow!("unusable signing key {kid}: {e}")))?;

    // Only the algorithm this token actually uses, having just checked it
    // against ALLOWED_ALGORITHMS above. jsonwebtoken requires every entry
    // in this list to match the key's family, so a mixed RSA/EC allowlist
    // here would reject an RSA token outright — and the allowlist has
    // already done its job, which is to refuse `none` and HMAC before any
    // key is looked up.
    let mut validation = Validation::new(header.alg);
    validation.algorithms = vec![header.alg];
    validation.leeway = leeway.as_secs();
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.set_issuer(&[cfg.issuer.trim_end_matches('/')]);
    validation.set_audience(audiences);

    let data =
        decode::<Value>(token, &key, &validation).map_err(|e| TokenError::Invalid(describe(&e)))?;
    Ok(data.claims)
}

/// Turn a jsonwebtoken error into something a person can act on.
fn describe(e: &jsonwebtoken::errors::Error) -> String {
    use jsonwebtoken::errors::ErrorKind;
    match e.kind() {
        ErrorKind::ExpiredSignature => "the token has expired".into(),
        ErrorKind::ImmatureSignature => "the token is not valid yet".into(),
        ErrorKind::InvalidIssuer => {
            "the token was issued by a different provider than auth.oidc.issuer".into()
        }
        ErrorKind::InvalidAudience => {
            "the token is not addressed to this server; check auth.oidc.audiences and the \
             audience scope requested at login"
                .into()
        }
        ErrorKind::InvalidSignature => "the token's signature does not verify".into(),
        other => format!("the token was rejected: {other:?}"),
    }
}

/// Ask the provider whether an opaque token is live, per RFC 7662.
async fn introspect(
    cfg: &OidcConfig,
    provider: &Provider,
    token: &str,
) -> Result<Value, TokenError> {
    let metadata = provider.metadata().await.map_err(TokenError::Unavailable)?;
    let endpoint = metadata.introspection_endpoint.clone().ok_or_else(|| {
        TokenError::Unavailable(anyhow::anyhow!(
            "auth.oidc.introspection is on, but the provider publishes no introspection_endpoint"
        ))
    })?;
    let secret = cfg.resolved_client_secret().ok_or_else(|| {
        TokenError::Unavailable(anyhow::anyhow!(
            "introspection needs auth.oidc.client_secret"
        ))
    })?;

    let resp = provider
        .http()
        .post(&endpoint)
        .basic_auth(&cfg.client_id, Some(secret))
        .form(&[("token", token), ("token_type_hint", "access_token")])
        .send()
        .await
        .map_err(|e| TokenError::Unavailable(anyhow::anyhow!("POST {endpoint}: {e}")))?;
    if !resp.status().is_success() {
        return Err(TokenError::Unavailable(anyhow::anyhow!(
            "the provider answered {} when asked to introspect a token",
            resp.status()
        )));
    }
    let claims: Value = resp.json().await.map_err(|e| {
        TokenError::Unavailable(anyhow::anyhow!("parsing the introspection reply: {e}"))
    })?;

    // `active` is the whole point of the call: everything else in the
    // response is advisory, and a token the provider has revoked comes
    // back with claims still attached.
    if claims.get("active").and_then(Value::as_bool) != Some(true) {
        return Err(TokenError::Invalid(
            "the provider says this token is not active".into(),
        ));
    }
    check_introspected_audience(cfg, &claims)?;
    Ok(claims)
}

/// Introspection answers "is it live", not "is it for you" — the audience
/// still has to be checked, or one tenant's token opens another's server.
fn check_introspected_audience(cfg: &OidcConfig, claims: &Value) -> Result<(), TokenError> {
    let accepted = cfg.accepted_audiences();
    let found: Vec<String> = match claims.get("aud") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    };
    if found.iter().any(|a| accepted.contains(a)) {
        return Ok(());
    }
    Err(TokenError::Invalid(format!(
        "the token is addressed to [{}], not to this server",
        found.join(", ")
    )))
}

/// The bearer token on a request, if there is one.
pub fn bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())?;
    let (scheme, rest) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| rest.trim())
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    fn headers(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_str(value).unwrap());
        h
    }

    #[test]
    fn a_bearer_token_is_read_whatever_the_case_of_the_scheme() {
        assert_eq!(bearer(&headers("Bearer abc")), Some("abc"));
        assert_eq!(bearer(&headers("bearer abc")), Some("abc"));
        assert_eq!(bearer(&headers("BEARER  abc ")), Some("abc"));
        assert_eq!(bearer(&headers("Basic abc")), None);
        assert_eq!(bearer(&headers("Bearer ")), None);
        assert_eq!(bearer(&HeaderMap::new()), None);
    }

    #[test]
    fn none_is_not_an_acceptable_algorithm() {
        // The allowlist is the defence against `alg: none` and against
        // an HMAC token signed with the provider's *public* key.
        assert!(!ALLOWED_ALGORITHMS.contains(&Algorithm::HS256));
        assert!(ALLOWED_ALGORITHMS.contains(&Algorithm::RS256));
    }

    #[test]
    fn an_introspected_token_for_another_audience_is_refused() {
        let cfg = OidcConfig {
            client_id: "otelview".into(),
            ..Default::default()
        };
        let mine = serde_json::json!({"active": true, "aud": ["otelview", "other"]});
        assert!(check_introspected_audience(&cfg, &mine).is_ok());

        let theirs = serde_json::json!({"active": true, "aud": "someone-else"});
        let err = check_introspected_audience(&cfg, &theirs).unwrap_err();
        assert!(matches!(err, TokenError::Invalid(_)));
        assert!(err.to_string().contains("someone-else"));

        let none = serde_json::json!({"active": true});
        assert!(check_introspected_audience(&cfg, &none).is_err());
    }
}
