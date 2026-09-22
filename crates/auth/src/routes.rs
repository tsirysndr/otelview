//! The endpoints a browser and an agent need.
//!
//! - `GET  /auth/info`     — public: which mode this instance is in.
//! - `GET  /auth/login`    — start the flow, redirect to the provider.
//! - `GET  /auth/callback` — come back with a code, leave with a session.
//! - `POST /auth/logout`   — drop the session, and end it at the provider.
//! - `GET  /auth/me`       — who am I, and what may I do.
//! - `GET  /.well-known/oauth-protected-resource` — RFC 9728, so an MCP
//!   client can discover which provider issues tokens for this server.
//!
//! `/auth/info` and the metadata document are deliberately public: a login
//! screen that requires a login cannot be used, and a client that has to
//! already hold a token to discover where tokens come from is a client
//! that has to be configured by hand.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::flow::{self, Pkce};
use crate::principal::{self, Credential};
use crate::session::random_id;
use crate::{Authenticator, Outcome};

/// What the UI needs to know before anyone has signed in.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AuthInfo {
    /// `none`, `token` or `oidc`.
    pub mode: &'static str,
    /// Where to send the browser to sign in, when the mode is `oidc`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_url: Option<&'static str>,
    /// The provider, for a "sign in with…" label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    /// Whether a static token is still accepted beside SSO, so the UI can
    /// offer that fallback rather than hiding it.
    pub static_token_accepted: bool,
}

pub fn router(auth: Arc<Authenticator>) -> Router {
    Router::new()
        .route("/auth/info", get(info))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout).get(logout))
        .route("/auth/me", get(me))
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .with_state(auth)
}

/// The same document, for an instance with no SSO configured — so the UI
/// can ask one question and get an answer either way.
pub fn info_without_sso(static_token_required: bool) -> AuthInfo {
    AuthInfo {
        mode: if static_token_required {
            "token"
        } else {
            "none"
        },
        login_url: None,
        issuer: None,
        static_token_accepted: static_token_required,
    }
}

async fn info(State(auth): State<Arc<Authenticator>>) -> Json<AuthInfo> {
    Json(AuthInfo {
        mode: "oidc",
        login_url: Some("/auth/login"),
        issuer: Some(auth.config.issuer.clone()),
        static_token_accepted: auth.config.allow_static_token,
    })
}

#[derive(Debug, Deserialize)]
struct LoginParams {
    /// Where to land after signing in. Must be a path on this server.
    #[serde(default)]
    return_to: Option<String>,
}

/// Only same-site paths are accepted as a return target.
///
/// An open redirect on a login endpoint is how a phishing page borrows a
/// real domain: `/auth/login?return_to=https://evil.example` would send
/// the user somewhere else with the site's own name in the address bar.
fn safe_return_to(raw: Option<String>) -> String {
    raw.filter(|r| r.starts_with('/') && !r.starts_with("//"))
        .unwrap_or_else(|| "/".to_string())
}

async fn login(
    State(auth): State<Arc<Authenticator>>,
    Query(params): Query<LoginParams>,
) -> Response {
    let pkce = Pkce::new();
    let nonce = random_id();
    let return_to = safe_return_to(params.return_to);
    let state = auth
        .pending
        .start(pkce.verifier.clone(), nonce.clone(), return_to);

    match flow::authorization_url(&auth.config, &auth.provider, &pkce, &state, &nonce).await {
        Ok(url) => {
            tracing::debug!("redirecting to the identity provider");
            Redirect::to(&url).into_response()
        }
        Err(e) => {
            tracing::error!(error = %format!("{e:#}"), "could not build the authorization URL");
            problem(
                StatusCode::BAD_GATEWAY,
                "This instance could not reach its identity provider. Check auth.oidc.issuer \
                 and that the provider is up.",
            )
        }
    }
}

#[derive(Debug, Deserialize)]
struct CallbackParams {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    /// The provider says no, in the OAuth error vocabulary.
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

async fn callback(
    State(auth): State<Arc<Authenticator>>,
    Query(params): Query<CallbackParams>,
) -> Response {
    if let Some(error) = params.error {
        let detail = params.error_description.unwrap_or_default();
        tracing::warn!(%error, %detail, "the identity provider refused the login");
        return problem(
            StatusCode::UNAUTHORIZED,
            &format!("The identity provider refused this login: {error}. {detail}"),
        );
    }

    let (Some(code), Some(state)) = (params.code, params.state) else {
        return problem(
            StatusCode::BAD_REQUEST,
            "This callback is missing its code or state. Start again from /auth/login.",
        );
    };

    // Spending the state is what proves this callback belongs to a login
    // this server started, in this browser.
    let Some(pending) = auth.pending.take(&state) else {
        tracing::warn!("callback presented an unknown or expired state");
        return problem(
            StatusCode::BAD_REQUEST,
            "This login has expired or was already used. Start again from /auth/login.",
        );
    };

    let tokens = match flow::exchange_code(&auth.config, &auth.provider, &code, &pending.verifier)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "the authorization code could not be exchanged");
            return problem(
                StatusCode::UNAUTHORIZED,
                "The identity provider would not exchange this login. It may have expired; \
                 start again from /auth/login.",
            );
        }
    };

    // The access token is verified exactly as an API caller's would be:
    // the browser path gets no shortcut, because a token that this server
    // would refuse on an API call has no business opening a session.
    let claims = match crate::token::verify(
        &auth.config,
        &auth.provider,
        std::time::Duration::from_secs(60),
        &tokens.access_token,
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "the provider's own access token did not verify");
            return problem(
                StatusCode::BAD_GATEWAY,
                "The token this provider issued did not verify. If it issues opaque tokens, \
                 set auth.oidc.introspection or request an audience scope.",
            );
        }
    };

    let mut principal = match principal::from_claims(&auth.config, &claims, Credential::Session) {
        Ok(p) => p,
        Err(denied) => {
            tracing::info!(reason = %denied, "a valid login was denied access");
            return problem(StatusCode::FORBIDDEN, &denied.to_string());
        }
    };

    // The id token is what says who this is, and its nonce is what ties
    // it to the login this server started. A provider that sent one and
    // cannot have it verified is a replay, not a formality.
    if let Some(id_token) = &tokens.id_token {
        match crate::token::verify_id_token(
            &auth.config,
            &auth.provider,
            std::time::Duration::from_secs(60),
            id_token,
            &pending.nonce,
        )
        .await
        {
            Ok(id_claims) => principal.enrich_display(&id_claims),
            Err(e) => {
                tracing::warn!(error = %e, "the id token did not verify");
                return problem(
                    StatusCode::UNAUTHORIZED,
                    "The identity token for this login did not verify. Start again from \
                     /auth/login.",
                );
            }
        }
    }

    tracing::info!(
        subject = %principal.subject,
        role = principal.role.as_str(),
        "signed in"
    );
    let cookie = auth.sessions.create(
        principal,
        Some(tokens.access_token),
        tokens.id_token.clone(),
    );
    (
        [(header::SET_COOKIE, auth.sessions.set_cookie(&cookie))],
        Redirect::to(&pending.return_to),
    )
        .into_response()
}

async fn logout(State(auth): State<Arc<Authenticator>>, headers: HeaderMap) -> Response {
    let ended = auth
        .sessions
        .from_headers(&headers)
        .and_then(|c| auth.sessions.remove(&c));
    if let Some(session) = &ended {
        tracing::info!(subject = %session.principal.subject, "signed out");
    }

    // Ending the session here but not at the provider means the next
    // /auth/login signs straight back in without asking — which does not
    // look like a logout to anyone.
    let provider_logout = flow::logout_url(
        &auth.config,
        &auth.provider,
        ended.as_ref().and_then(|s| s.id_token.as_deref()),
    )
    .await
    .unwrap_or(None);

    let clear = auth.sessions.clear_cookie();
    match provider_logout {
        Some(url) => ([(header::SET_COOKIE, clear)], Redirect::to(&url)).into_response(),
        None => (
            [(header::SET_COOKIE, clear)],
            Json(json!({"signed_out": true})),
        )
            .into_response(),
    }
}

async fn me(State(auth): State<Arc<Authenticator>>, headers: HeaderMap) -> Response {
    match auth.authenticate(&headers).await {
        Outcome::Authenticated(p) => Json(json!({
            "authenticated": true,
            "subject": p.subject,
            "name": p.name,
            "email": p.email,
            "organization": p.organization,
            "roles": p.roles,
            "role": p.role,
            "via": p.via,
            "permissions": {
                "read_telemetry": p.can(crate::Permission::ReadTelemetry),
                "read_config": p.can(crate::Permission::ReadConfig),
                "use_mcp": p.can(crate::Permission::UseMcp),
                "administer": p.can(crate::Permission::Administer),
            }
        }))
        .into_response(),
        Outcome::Anonymous => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"authenticated": false})),
        )
            .into_response(),
        Outcome::Rejected(reason) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"authenticated": false, "reason": reason})),
        )
            .into_response(),
        Outcome::Unavailable(reason) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"authenticated": false, "reason": reason})),
        )
            .into_response(),
    }
}

/// RFC 9728 protected-resource metadata.
///
/// An MCP client that gets a 401 reads the `resource_metadata` URL out of
/// `WWW-Authenticate`, fetches this, and learns which authorization server
/// to go to. That is the difference between an agent that can sign itself
/// in and one that needs a human to paste a token.
async fn protected_resource_metadata(
    State(auth): State<Arc<Authenticator>>,
) -> Json<serde_json::Value> {
    Json(json!({
        "resource": auth.config.redirect_url.split("/auth/callback").next().unwrap_or_default(),
        "authorization_servers": [auth.config.issuer],
        "scopes_supported": auth.config.scopes,
        "bearer_methods_supported": ["header"],
    }))
}

/// An error a person reads in a browser, not a JSON body they never see.
fn problem(status: StatusCode, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        format!(
            "<!doctype html><meta charset=utf-8><title>otelview — sign-in</title>\
             <style>body{{font:14px/1.6 system-ui,sans-serif;max-width:34rem;margin:20vh auto;\
             padding:0 1.5rem;color:#ddd;background:#16161a}}a{{color:#ff2e97}}\
             h1{{font-size:1rem;letter-spacing:.18em;text-transform:uppercase}}</style>\
             <h1>otelview</h1><p>{}</p><p><a href=\"/auth/login\">Try again</a></p>",
            html_escape(message)
        ),
    )
        .into_response()
}

/// The message is ours, but it quotes the provider, and the provider
/// quotes the query string.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_return_target_must_be_a_path_on_this_server() {
        assert_eq!(safe_return_to(Some("/traces".into())), "/traces");
        assert_eq!(safe_return_to(None), "/");
        // Open-redirect attempts all land on the root instead.
        assert_eq!(safe_return_to(Some("https://evil.example".into())), "/");
        assert_eq!(safe_return_to(Some("//evil.example".into())), "/");
        assert_eq!(safe_return_to(Some("javascript:alert(1)".into())), "/");
    }

    #[test]
    fn provider_messages_are_escaped_before_they_reach_a_page() {
        let out = html_escape("<script>alert('x')</script> & \"quotes\"");
        assert!(!out.contains("<script>"));
        assert!(out.contains("&lt;script&gt;"));
        assert!(out.contains("&amp;"));
        assert!(out.contains("&quot;"));
    }

    #[test]
    fn the_no_sso_info_document_still_tells_the_ui_what_to_do() {
        let open = info_without_sso(false);
        assert_eq!(open.mode, "none");
        assert!(open.login_url.is_none());

        let tokened = info_without_sso(true);
        assert_eq!(tokened.mode, "token");
        assert!(tokened.static_token_accepted);
    }
}
