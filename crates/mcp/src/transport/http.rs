//! Streamable HTTP: one endpoint, POST a JSON-RPC message, get one back.
//!
//! This is the transport a remote client uses, and the one mounted beside
//! the web UI so a running otelview *is* an MCP server. It is also the only
//! transport that can be reached by anyone but the user who launched the
//! process, so it is the only one that carries a token — [`super::stdio`]
//! talks over a pipe to whoever started the process and has nothing to
//! authenticate.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use otelview_auth::{Authenticator, Outcome, Permission};

use crate::Mcp;

/// What a request must present to be answered.
#[derive(Debug, Clone)]
pub struct Auth {
    /// `None` leaves the endpoint open — correct for a loopback-only
    /// instance, and refused by [`crate::transport::http::serve`] for
    /// anything else.
    pub token: Option<String>,
    /// Header accepted besides `Authorization: Bearer …`, matching the
    /// ingest/API header so one token works everywhere.
    pub header: String,
    /// Single sign-on. When set, an agent may present an OIDC access
    /// token instead of the shared one — which is what lets a human's
    /// own identity, roles and MFA follow them into an agent session
    /// rather than everyone sharing one secret.
    pub oidc: Option<Arc<Authenticator>>,
}

impl Auth {
    pub fn open() -> Self {
        Self {
            token: None,
            header: "x-otelview-token".into(),
            oidc: None,
        }
    }

    pub fn bearer(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            ..Self::open()
        }
    }

    pub fn with_oidc(mut self, oidc: Option<Arc<Authenticator>>) -> Self {
        self.oidc = oidc;
        self
    }

    /// True when *something* guards this endpoint.
    pub fn enabled(&self) -> bool {
        self.token.as_deref().is_some_and(|t| !t.is_empty()) || self.oidc.is_some()
    }

    /// Whether this request may use the endpoint.
    ///
    /// The shared token first because it is a byte comparison, then the
    /// identity provider, which may cost a network call the first time it
    /// sees a signing key.
    async fn admits_request(&self, headers: &HeaderMap) -> Admission {
        if self.static_token_matches(headers) {
            return Admission::Allowed;
        }
        let Some(oidc) = &self.oidc else {
            return if self.token.is_some() {
                Admission::Denied
            } else {
                // No token and no provider: the endpoint is open, and
                // `serve` has already refused to expose it beyond
                // loopback in that state.
                Admission::Allowed
            };
        };
        match oidc.authenticate(headers).await {
            Outcome::Authenticated(principal) => {
                if principal.can(Permission::UseMcp) {
                    tracing::debug!(subject = %principal.label(), "mcp request authenticated");
                    Admission::Allowed
                } else {
                    Admission::Forbidden("this account may not use MCP on this instance".into())
                }
            }
            Outcome::Anonymous | Outcome::Rejected(_) => Admission::Denied,
            Outcome::Unavailable(reason) => Admission::Unavailable(reason),
        }
    }

    /// True when `headers` carries the expected shared token, in either
    /// accepted form.
    fn static_token_matches(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = self.token.as_deref().filter(|t| !t.is_empty()) else {
            return false;
        };
        let presented = headers
            .get(&self.header)
            .and_then(|v| v.to_str().ok())
            .or_else(|| {
                headers
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| {
                        // Case-insensitive scheme: clients spell it Bearer,
                        // bearer and BEARER, and all three are valid.
                        let (scheme, rest) = v.split_once(' ')?;
                        scheme.eq_ignore_ascii_case("bearer").then_some(rest.trim())
                    })
            });
        presented.is_some_and(|p| constant_time_eq(p.as_bytes(), expected.as_bytes()))
    }
}

/// Whether a browser on this origin may talk to the endpoint.
///
/// A real MCP client sends no `Origin` at all, so the header's presence
/// means a web page is calling. The attack it enables is DNS rebinding: a
/// page on any domain resolves that domain to 127.0.0.1 and then reads a
/// local endpoint that only ever expected local callers — which is the
/// whole telemetry store. Only pages served from loopback are allowed,
/// and a rebound page's origin is never loopback.
fn origin_admitted(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let host = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin);
    // Strip the port, taking care with the bracketed IPv6 form.
    let host = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(rest),
        None => host.split(':').next().unwrap_or(host),
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Compare without returning early on the first differing byte, so the
/// time taken says nothing about how much of the token was right.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The answer to "may this request in?".
enum Admission {
    Allowed,
    /// No usable credential: a 401, with a challenge telling the caller
    /// where to get one.
    Denied,
    /// A good credential without the rights: a 403, which a caller must
    /// not retry by fetching another token.
    Forbidden(String),
    /// The provider could not be reached.
    Unavailable(String),
}

#[derive(Clone)]
struct HttpState {
    mcp: Mcp,
    auth: Arc<Auth>,
}

/// An axum router serving MCP at `path`.
///
/// Merge it into another router to serve MCP beside something else, or
/// hand it to [`serve`] to run it on its own.
pub fn router(mcp: Mcp, path: &str, auth: Auth) -> Router {
    let path = normalize(path);
    Router::new()
        // GET is where a streamable-HTTP client would open an SSE stream
        // and DELETE where it would end a session. This server has neither:
        // every response is complete when the POST returns. 405 is the
        // spec's answer for both, and is what stops a client waiting on a
        // stream that will never produce anything.
        .route(&path, post(handle).fallback(unsupported))
        .with_state(HttpState {
            mcp,
            auth: Arc::new(auth),
        })
}

/// Leading slash, no trailing slash: `mcp`, `/mcp` and `/mcp/` all name the
/// same endpoint, and a config typo should not silently move it.
fn normalize(path: &str) -> String {
    let trimmed = path.trim().trim_matches('/');
    if trimmed.is_empty() {
        "/mcp".to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// Serve MCP on its own listener.
///
/// Refuses to bind a non-loopback address without a token: an open MCP
/// endpoint on a public interface hands the whole telemetry store to
/// anyone who finds it, and the failure mode of a silent default is that
/// nobody notices until it matters.
pub async fn serve(mcp: Mcp, addr: SocketAddr, path: &str, auth: Auth) -> Result<()> {
    if !addr.ip().is_loopback() && !auth.enabled() {
        anyhow::bail!(
            "refusing to serve MCP on {addr} without a token: set `mcp.token` in the \
             config or export OTELVIEW_MCP_TOKEN, or bind a loopback address"
        );
    }
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding the MCP server to {addr}"))?;
    let path = normalize(path);
    tracing::info!(
        "MCP server listening on http://{addr}{path} ({})",
        if auth.enabled() {
            "bearer token required"
        } else {
            "open, loopback only"
        }
    );
    axum::serve(listener, router(mcp, &path, auth))
        .await
        .context("MCP server failed")
}

async fn handle(State(state): State<HttpState>, headers: HeaderMap, body: String) -> Response {
    // Refusals are logged at warn and everything else at trace: a rejected
    // request is either a misconfigured client or someone trying, and both
    // are worth finding in a log. A served one is already accounted for by
    // the per-method lines the protocol layer emits.
    if !origin_admitted(&headers) {
        tracing::warn!(
            origin = headers
                .get(header::ORIGIN)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("?"),
            "refused an MCP request from a foreign browser origin"
        );
        return (
            StatusCode::FORBIDDEN,
            "this MCP endpoint does not accept browser requests from other origins",
        )
            .into_response();
    }
    match state.auth.admits_request(&headers).await {
        Admission::Allowed => {}
        Admission::Denied => {
            // Which credential was offered, never what it was.
            tracing::warn!(
                presented = headers.contains_key(header::AUTHORIZATION)
                    || headers.contains_key(&state.auth.header),
                "refused an MCP request with a missing or invalid token"
            );
            return unauthorized(&state.auth);
        }
        Admission::Forbidden(reason) => {
            tracing::warn!(%reason, "refused an MCP request on authorization");
            return (StatusCode::FORBIDDEN, reason).into_response();
        }
        Admission::Unavailable(reason) => {
            tracing::error!(%reason, "the identity provider could not be reached");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("the identity provider could not be reached: {reason}"),
            )
                .into_response();
        }
    }
    tracing::trace!(bytes = body.len(), "mcp message in over http");
    match state.mcp.handle(&body).await {
        Some(response) => ([(header::CONTENT_TYPE, "application/json")], response).into_response(),
        // A notification has no reply, and the spec asks for exactly this.
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn unsupported(method: axum::http::Method) -> Response {
    // Worth a line: a client doing this is expecting an SSE stream or a
    // session, and will otherwise look like it simply never connected.
    tracing::debug!(%method, "refused a non-POST MCP request");
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST")],
        "this MCP endpoint accepts POST only: it has no server-initiated \
         stream and no sessions to delete",
    )
        .into_response()
}

fn unauthorized(auth: &Auth) -> Response {
    // With a provider configured the challenge points at RFC 9728
    // metadata, which is how an MCP client discovers where to get a token
    // instead of needing one pasted in by hand.
    let challenge = match &auth.oidc {
        Some(oidc) => oidc.challenge("/.well-known/oauth-protected-resource"),
        None => "Bearer realm=\"otelview\", error=\"invalid_token\"".to_string(),
    };
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, challenge)],
        "missing or invalid MCP credentials",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use crate::testing::sample_otel;

    async fn app(auth: Auth) -> Router {
        router(Mcp::new(sample_otel().await), "/mcp", auth)
    }

    async fn post_with(
        app: Router,
        headers: Vec<(&str, &str)>,
        body: Value,
    ) -> (StatusCode, String) {
        let mut req = Request::post("/mcp");
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let resp = app
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn a_request_gets_a_json_rpc_response() {
        let (status, body) = post_with(
            app(Auth::open()).await,
            vec![],
            json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["id"], 1);
    }

    #[tokio::test]
    async fn a_notification_is_accepted_with_no_body() {
        let (status, body) = post_with(
            app(Auth::open()).await,
            vec![],
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn without_a_token_a_guarded_endpoint_says_401() {
        let (status, _) = post_with(
            app(Auth::bearer("sekret")).await,
            vec![],
            json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_wrong_token_is_also_401() {
        let (status, _) = post_with(
            app(Auth::bearer("sekret")).await,
            vec![("authorization", "Bearer nope")],
            json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_token_is_accepted_as_bearer_or_as_the_header() {
        for headers in [
            vec![("authorization", "Bearer sekret")],
            vec![("authorization", "bearer sekret")],
            vec![("x-otelview-token", "sekret")],
        ] {
            let (status, _) = post_with(
                app(Auth::bearer("sekret")).await,
                headers.clone(),
                json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "rejected {headers:?}");
        }
    }

    #[tokio::test]
    async fn get_is_refused_rather_than_left_hanging() {
        let resp = app(Auth::open())
            .await
            .oneshot(Request::get("/mcp").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn an_open_endpoint_refuses_a_public_address() {
        let mcp = Mcp::new(sample_otel().await);
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let err = serve(mcp, addr, "/mcp", Auth::open()).await.unwrap_err();
        assert!(err.to_string().contains("without a token"), "{err}");
    }

    #[test]
    fn paths_normalize_to_one_spelling() {
        assert_eq!(normalize("mcp"), "/mcp");
        assert_eq!(normalize("/mcp/"), "/mcp");
        assert_eq!(normalize("  "), "/mcp");
        assert_eq!(normalize("/v1/mcp"), "/v1/mcp");
    }

    #[tokio::test]
    async fn a_page_on_another_origin_is_refused() {
        // The DNS-rebinding case: a token would stop it, but the default
        // endpoint is open on loopback and this is what stands in the way.
        let (status, _) = post_with(
            app(Auth::open()).await,
            vec![("origin", "https://evil.example")],
            json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_local_page_and_a_non_browser_client_are_let_through() {
        for headers in [
            vec![("origin", "http://localhost:5173")],
            vec![("origin", "http://127.0.0.1:4319")],
            // A real MCP client sends no Origin at all.
            vec![],
        ] {
            let (status, _) = post_with(
                app(Auth::open()).await,
                headers.clone(),
                json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "rejected {headers:?}");
        }
    }

    #[test]
    fn origins_are_matched_by_host_not_by_prefix() {
        let origin = |v: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::ORIGIN, v.parse().unwrap());
            h
        };
        assert!(origin_admitted(&origin("http://localhost")));
        assert!(origin_admitted(&origin("http://[::1]:4319")));
        assert!(origin_admitted(&origin("http://127.0.0.2:4319")));
        // Names that merely start with a loopback spelling are not it.
        assert!(!origin_admitted(&origin("http://localhost.evil.example")));
        assert!(!origin_admitted(&origin("http://127.0.0.1.evil.example")));
        assert!(!origin_admitted(&origin("https://otelview.internal")));
    }

    #[test]
    fn token_comparison_rejects_a_prefix() {
        assert!(!constant_time_eq(b"sek", b"sekret"));
        assert!(constant_time_eq(b"sekret", b"sekret"));
    }
}
