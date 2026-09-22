//! Single sign-on for otelview: an OpenID Connect relying party.
//!
//! otelview verifies tokens and reads claims. It does not store passwords,
//! enrol second factors, register passkeys or speak SAML — all of that
//! belongs to the identity provider it delegates to, and otelview inherits
//! every bit of it for free. Point this at a Zitadel with SAML federation
//! and passkeys turned on, and otelview has SAML federation and passkeys.
//!
//! What is here:
//!
//! - [`provider`] — discovery and signing keys, cached and rotated.
//! - [`token`] — is this access token genuine (JWT, or introspection).
//! - [`principal`] — who it belongs to and what they may do (RBAC).
//! - [`session`] — browser sessions behind a signed, HttpOnly cookie.
//! - [`flow`] — authorization code + PKCE.
//! - [`routes`] — `/auth/*` and the OAuth metadata agents discover.
//!
//! All of it is inert unless `auth.oidc.enabled` is set: [`Authenticator`]
//! is an `Option` at every call site, and `None` is a local dev instance
//! with no identity provider in sight.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::http::HeaderMap;
use otelview_config::{parse_interval, Config, OidcConfig};

pub mod flow;
pub mod principal;
pub mod provider;
pub mod routes;
pub mod session;
pub mod token;

#[cfg(feature = "testing")]
pub mod testing;

pub use principal::{Credential, Denied, Permission, Principal, Role};
pub use session::SessionStore;

/// Everything needed to answer "who is this, and may they?".
pub struct Authenticator {
    pub config: OidcConfig,
    pub provider: provider::Provider,
    pub sessions: SessionStore,
    pub pending: flow::PendingLogins,
    leeway: Duration,
    /// The static token that still opens the door, when one is set and
    /// `allow_static_token` has not turned it off.
    static_token: Option<String>,
}

/// Prints what a log line may show and nothing else: the static token and
/// the session table are exactly what must never appear in a debug dump.
impl std::fmt::Debug for Authenticator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authenticator")
            .field("issuer", &self.config.issuer)
            .field("client_id", &self.config.client_id)
            .field("introspection", &self.config.introspection)
            .field("sessions", &self.sessions.len())
            .field("static_token", &self.static_token.is_some())
            .finish()
    }
}

/// The outcome of looking at a request's credentials.
#[derive(Debug)]
pub enum Outcome {
    /// Nobody is asking — no cookie, no token.
    Anonymous,
    /// Someone is, and here they are.
    Authenticated(Box<Principal>),
    /// A credential was presented and it is not good.
    Rejected(String),
    /// The provider could not be reached to find out.
    Unavailable(String),
}

impl Authenticator {
    /// Build one from config, or `None` when SSO is off.
    ///
    /// Returns an error only for configuration that cannot work at all;
    /// the provider itself is contacted lazily, so a provider that is
    /// briefly down does not stop otelview from starting.
    pub fn from_config(cfg: &Config) -> Result<Option<Arc<Self>>> {
        let oidc = &cfg.auth.oidc;
        if !oidc.enabled {
            return Ok(None);
        }
        oidc.validate()?;

        let refresh = parse_interval(&oidc.jwks_refresh_interval)?;
        let leeway = parse_interval(&oidc.clock_skew_leeway)?;
        let session_ttl = parse_interval(&oidc.session_ttl)?;

        let static_token = oidc
            .allow_static_token
            .then(|| cfg.mcp.resolved_token(cfg))
            .flatten();

        tracing::info!(
            issuer = %oidc.issuer,
            client_id = %oidc.client_id,
            introspection = oidc.introspection,
            viewer_roles = oidc.viewer_roles.len(),
            admin_roles = oidc.admin_roles.len(),
            static_token = static_token.is_some(),
            "single sign-on enabled"
        );
        if oidc.viewer_roles.is_empty() && oidc.admin_roles.is_empty() {
            tracing::warn!(
                "auth.oidc has no viewer_roles or admin_roles, so every account the provider \
                 admits can read this instance's telemetry"
            );
        }
        if oidc.allow_insecure_issuer && oidc.issuer.starts_with("http://") {
            tracing::warn!(
                issuer = %oidc.issuer,
                "auth.oidc.allow_insecure_issuer is on: the authorization code and every \
                 token cross the network in clear text"
            );
        }
        if !oidc.secure_cookies {
            tracing::warn!(
                "auth.oidc.secure_cookies is off: the session cookie will travel over plain http"
            );
        }

        Ok(Some(Arc::new(Self {
            provider: provider::Provider::new(oidc.discovery_url(), oidc.issuer.clone(), refresh)?,
            sessions: SessionStore::new(
                session_ttl,
                oidc.session_cookie.clone(),
                oidc.secure_cookies,
            ),
            pending: flow::PendingLogins::new(),
            config: oidc.clone(),
            leeway,
            static_token,
        })))
    }

    /// Identify the caller behind a request.
    ///
    /// Order matters: the session cookie first because that is the browser
    /// and the common case, then a bearer token for API and agent callers,
    /// then the static token last so that a deployment moving to SSO does
    /// not break its own CI overnight.
    pub async fn authenticate(&self, headers: &HeaderMap) -> Outcome {
        if let Some(cookie) = self.sessions.from_headers(headers) {
            match self.sessions.get(&cookie) {
                Some(session) => return Outcome::Authenticated(Box::new(session.principal)),
                // A cookie that no longer resolves is an expired or
                // restarted session, not an attack: fall through so a
                // bearer token on the same request still works.
                None => tracing::debug!("session cookie did not resolve"),
            }
        }

        if let Some(presented) = token::bearer(headers) {
            if let Some(expected) = &self.static_token {
                if session::constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
                    return Outcome::Authenticated(Box::new(Principal::static_token()));
                }
            }
            return self.authenticate_token(presented).await;
        }

        Outcome::Anonymous
    }

    /// Verify a bearer token and map it to a principal.
    pub async fn authenticate_token(&self, presented: &str) -> Outcome {
        match token::verify(&self.config, &self.provider, self.leeway, presented).await {
            Ok(claims) => {
                match principal::from_claims(&self.config, &claims, Credential::BearerToken) {
                    Ok(p) => Outcome::Authenticated(Box::new(p)),
                    Err(denied) => Outcome::Rejected(denied.to_string()),
                }
            }
            Err(token::TokenError::Invalid(msg)) => Outcome::Rejected(msg),
            Err(token::TokenError::Unavailable(e)) => Outcome::Unavailable(format!("{e:#}")),
        }
    }

    /// The `WWW-Authenticate` value for a 401.
    ///
    /// Carries the resource metadata URL from RFC 9728, which is how an
    /// MCP client discovers *which* provider to go and get a token from
    /// rather than having it configured out of band.
    pub fn challenge(&self, resource_metadata_url: &str) -> String {
        format!("Bearer realm=\"otelview\", resource_metadata=\"{resource_metadata_url}\"",)
    }
}

/// Load the authenticator for a config, logging what it means when it is
/// off. A helper so every binary reports the same thing.
pub fn authenticator(cfg: &Config) -> Result<Option<Arc<Authenticator>>> {
    Authenticator::from_config(cfg).context("configuring single sign-on")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sso_off_means_no_authenticator_at_all() {
        let cfg = Config::default();
        assert!(Authenticator::from_config(&cfg).unwrap().is_none());
    }

    #[test]
    fn enabling_sso_without_the_required_fields_fails_loudly() {
        let mut cfg = Config::default();
        cfg.auth.oidc.enabled = true;
        // `Authenticator` holds an http client and carries no Debug, so
        // the error comes out of a match rather than `unwrap_err`.
        let err = match Authenticator::from_config(&cfg) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("expected the half-configured provider to be refused"),
        };
        assert!(err.contains("auth.oidc.issuer"), "{err}");
    }

    fn configured() -> Config {
        let mut cfg = Config::default();
        cfg.auth.oidc.enabled = true;
        cfg.auth.oidc.issuer = "https://auth.example.com".into();
        cfg.auth.oidc.client_id = "otelview".into();
        cfg.auth.oidc.redirect_url = "https://otelview.example.com/auth/callback".into();
        cfg
    }

    #[tokio::test]
    async fn a_request_with_no_credentials_is_anonymous() {
        let auth = Authenticator::from_config(&configured()).unwrap().unwrap();
        assert!(matches!(
            auth.authenticate(&HeaderMap::new()).await,
            Outcome::Anonymous
        ));
    }

    /// CI keeps working when a deployment turns SSO on, unless it says
    /// otherwise.
    #[tokio::test]
    async fn the_static_token_still_opens_the_door() {
        let mut cfg = configured();
        cfg.ui.token = Some("ci-token".into());
        let auth = Authenticator::from_config(&cfg).unwrap().unwrap();

        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer ci-token".parse().unwrap());
        match auth.authenticate(&headers).await {
            Outcome::Authenticated(p) => {
                assert_eq!(p.via, Credential::StaticToken);
                assert!(p.is_admin());
            }
            other => panic!("expected the static token to be accepted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_static_token_can_be_turned_off() {
        let mut cfg = configured();
        cfg.ui.token = Some("ci-token".into());
        cfg.auth.oidc.allow_static_token = false;
        let auth = Authenticator::from_config(&cfg).unwrap().unwrap();

        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer ci-token".parse().unwrap());
        // Now it is just an unrecognised token, and with no provider
        // reachable in a unit test it cannot be anything else.
        assert!(!matches!(
            auth.authenticate(&headers).await,
            Outcome::Authenticated(_)
        ));
    }

    #[test]
    fn the_challenge_points_at_the_resource_metadata() {
        let auth = Authenticator::from_config(&configured()).unwrap().unwrap();
        let challenge =
            auth.challenge("https://otelview.example.com/.well-known/oauth-protected-resource");
        assert!(challenge.starts_with("Bearer realm=\"otelview\""));
        assert!(challenge.contains("resource_metadata=\"https://otelview.example.com"));
    }
}
