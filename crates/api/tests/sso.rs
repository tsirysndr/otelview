//! The query API behind single sign-on.
//!
//! The auth crate proves the flow works; this proves the flow is actually
//! *in front of* the telemetry — that `/api/traces` is unreachable without
//! a credential, that a viewer and an admin are told apart, and that none
//! of it changes when SSO is off.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use otelview_api::{router, router_with, ServeOptions};
use otelview_auth::testing::MockIdp;
use otelview_auth::Authenticator;
use otelview_config::{Config, MemoryConfig};
use otelview_storage::memory::MemoryStorage;
use otelview_storage::DynStorage;
use serde_json::{json, Value};
use tower::ServiceExt;

fn storage() -> DynStorage {
    Arc::new(MemoryStorage::new(&MemoryConfig::default()))
}

fn config(idp: &MockIdp) -> Config {
    let mut cfg = Config::default();
    cfg.auth.oidc.enabled = true;
    cfg.auth.oidc.issuer = idp.base.clone();
    cfg.auth.oidc.client_id = "otelview".into();
    cfg.auth.oidc.redirect_url = "http://127.0.0.1:4319/auth/callback".into();
    cfg.auth.oidc.viewer_roles = vec!["otelview.viewer".into()];
    cfg.auth.oidc.admin_roles = vec!["otelview.admin".into()];
    cfg.auth.oidc.secure_cookies = false;
    cfg
}

fn app(cfg: &Config) -> Router {
    let auth = Authenticator::from_config(cfg).unwrap();
    router_with(cfg, storage(), ServeOptions::default().with_auth(auth))
}

async fn get(
    app: &Router,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut req = Request::get(uri);
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

fn viewer(idp: &MockIdp) -> String {
    idp.mint(idp.user_claims("viewer-1", "otelview", json!({"otelview.viewer": {}})))
}

fn admin(idp: &MockIdp) -> String {
    idp.mint(idp.user_claims("admin-1", "otelview", json!({"otelview.admin": {}})))
}

#[tokio::test]
async fn telemetry_is_unreachable_without_a_credential() {
    let idp = MockIdp::start().await;
    let app = app(&config(&idp));

    for path in [
        "/api/traces",
        "/api/logs",
        "/api/metrics",
        "/api/services",
        "/api/stats",
        "/api/service-graph",
    ] {
        let (status, headers, _) = get(&app, path, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path} was not guarded");
        // The challenge tells a client where to get a token, which is what
        // makes an agent able to sign itself in.
        let challenge = headers["www-authenticate"].to_str().unwrap();
        assert!(challenge.contains("resource_metadata="), "{challenge}");
    }
}

#[tokio::test]
async fn a_valid_token_reaches_the_telemetry() {
    let idp = MockIdp::start().await;
    let app = app(&config(&idp));

    let (status, _, body) = get(&app, "/api/services", Some(&viewer(&idp))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!([]));
}

/// The visible half of RBAC: same instance, same token machinery, two
/// different answers depending on the role in the token.
#[tokio::test]
async fn reading_the_configuration_needs_an_admin() {
    let idp = MockIdp::start().await;
    let app = app(&config(&idp));

    let (status, _, body) = get(&app, "/api/config", Some(&viewer(&idp))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("admin role"), "{body}");

    let (status, _, body) = get(&app, "/api/config", Some(&admin(&idp))).await;
    assert_eq!(status, StatusCode::OK);
    // And it is still the redacted copy.
    let cfg: Value = serde_json::from_str(&body).unwrap();
    assert!(cfg["storage"].is_object());
}

#[tokio::test]
async fn a_token_from_elsewhere_does_not_open_the_api() {
    let idp = MockIdp::start().await;
    let app = app(&config(&idp));

    let (status, _, _) = get(&app, "/api/services", Some("not-a-token")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Signed properly, but for another audience.
    let wrong = idp.mint(idp.user_claims("ada", "another-app", json!({"otelview.admin": {}})));
    let (status, _, _) = get(&app, "/api/services", Some(&wrong)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_account_with_no_role_cannot_read_telemetry() {
    let idp = MockIdp::start().await;
    let app = app(&config(&idp));

    let outsider = idp.mint(idp.user_claims("mallory", "otelview", json!({"other.app": {}})));
    let (status, _, _) = get(&app, "/api/traces", Some(&outsider)).await;
    // Authenticated, and still not allowed: a 401 would invite a retry
    // with a fresh token, which would change nothing.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_login_endpoints_are_reachable_without_signing_in() {
    let idp = MockIdp::start().await;
    let app = app(&config(&idp));

    let (status, _, body) = get(&app, "/auth/info", None).await;
    assert_eq!(status, StatusCode::OK);
    let info: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["mode"], "oidc");
    assert_eq!(info["issuer"], idp.base);

    let (status, _, _) = get(&app, "/.well-known/oauth-protected-resource", None).await;
    assert_eq!(status, StatusCode::OK);

    // The SPA shell is public too, or the login screen could not load.
    let (status, _, _) = get(&app, "/", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_static_token_still_works_beside_sso() {
    let idp = MockIdp::start().await;
    let mut cfg = config(&idp);
    cfg.ui.token = Some("ci-token".into());
    let app = app(&cfg);

    let (status, _, _) = get(&app, "/api/services", Some("ci-token")).await;
    assert_eq!(status, StatusCode::OK);
    // And it is an admin, so CI can read the config too.
    let (status, _, _) = get(&app, "/api/config", Some("ci-token")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_static_token_can_be_switched_off() {
    let idp = MockIdp::start().await;
    let mut cfg = config(&idp);
    cfg.ui.token = Some("ci-token".into());
    cfg.auth.oidc.allow_static_token = false;
    let app = app(&cfg);

    let (status, _, _) = get(&app, "/api/services", Some("ci-token")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Nothing above applies to an instance that has not turned SSO on.
#[tokio::test]
async fn an_instance_without_sso_is_unchanged() {
    let cfg = Config::default();
    let app = router(&cfg, storage());

    let (status, _, _) = get(&app, "/api/services", None).await;
    assert_eq!(status, StatusCode::OK, "local dev must need no credentials");

    let (status, _, _) = get(&app, "/api/config", None).await;
    assert_eq!(status, StatusCode::OK, "and no roles either");

    // The UI still gets an answer to "how do I sign in here?".
    let (status, _, body) = get(&app, "/auth/info", None).await;
    assert_eq!(status, StatusCode::OK);
    let info: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["mode"], "none");
}

#[tokio::test]
async fn an_instance_with_only_a_static_token_reports_that_mode() {
    let mut cfg = Config::default();
    cfg.ui.token = Some("sekret".into());
    let app = router(&cfg, storage());

    let (status, _, body) = get(&app, "/auth/info", None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the mode is public, or nobody can log in"
    );
    let info: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["mode"], "token");
    assert_eq!(info["static_token_accepted"], true);

    let (status, _, _) = get(&app, "/api/services", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = get(&app, "/api/services", Some("sekret")).await;
    assert_eq!(status, StatusCode::OK);
}
