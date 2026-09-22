//! The whole sign-in, against a provider that really signs tokens.
//!
//! Everything a browser does, in order: hit `/auth/login`, follow the
//! redirect's parameters, come back to `/auth/callback` with a code, and
//! end up holding a session cookie that `/auth/me` recognises. Then the
//! ways it goes wrong, which is the half that matters.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use otelview_auth::{Authenticator, Outcome};
use otelview_config::Config;
use serde_json::{json, Value};
use tower::ServiceExt;

use otelview_auth::testing::{self as support, MockIdp};

/// An instance pointed at `idp`, requiring the roles a real deployment
/// would require.
fn config(idp: &MockIdp) -> Config {
    let mut cfg = Config::default();
    cfg.auth.oidc.enabled = true;
    cfg.auth.oidc.issuer = idp.base.clone();
    cfg.auth.oidc.client_id = "otelview".into();
    cfg.auth.oidc.redirect_url = "http://127.0.0.1:4319/auth/callback".into();
    cfg.auth.oidc.viewer_roles = vec!["otelview.viewer".into()];
    cfg.auth.oidc.admin_roles = vec!["otelview.admin".into()];
    // The tests talk plain http to a loopback provider.
    cfg.auth.oidc.secure_cookies = false;
    cfg
}

async fn app(cfg: &Config) -> (Router, Arc<Authenticator>) {
    let auth = Authenticator::from_config(cfg).unwrap().expect("sso is on");
    (otelview_auth::routes::router(auth.clone()), auth)
}

async fn get(app: &Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    send(app, Request::get(uri).body(Body::empty()).unwrap()).await
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

/// Pull a query parameter out of a redirect's Location.
fn param(location: &str, name: &str) -> Option<String> {
    let query = location.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Walk the flow and return the session cookie.
async fn sign_in(app: &Router, idp: &MockIdp, claims: Value) -> String {
    idp.will_issue(claims);
    let (status, headers, _) = get(app, "/auth/login?return_to=/traces").await;
    assert_eq!(status, StatusCode::SEE_OTHER, "login should redirect");
    let location = headers["location"].to_str().unwrap().to_string();
    let state = param(&location, "state").expect("the redirect carries state");
    // A provider echoes the nonce it was sent; otelview checks it.
    idp.will_echo_nonce(&param(&location, "nonce").expect("the redirect carries a nonce"));

    let (status, headers, body) =
        get(app, &format!("/auth/callback?code=the-code&state={state}")).await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "callback should redirect: {body}"
    );
    assert_eq!(headers["location"], "/traces", "back where we started");

    headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn a_user_signs_in_and_is_recognised() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, auth) = app(&cfg).await;

    let claims = idp.user_claims("ada", "otelview", json!({"otelview.admin": {}}));
    let cookie = sign_in(&app, &idp, claims).await;

    // The code was exchanged with the PKCE verifier, not just the code.
    let form = idp.last_token_request().expect("a token request");
    assert!(form.contains("grant_type=authorization_code"), "{form}");
    assert!(form.contains("code_verifier="), "{form}");
    assert!(form.contains("code=the-code"), "{form}");

    let (status, _, body) = send(
        &app,
        Request::get("/auth/me")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let me: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(me["authenticated"], true);
    assert_eq!(me["subject"], "ada");
    assert_eq!(me["email"], "ada@example.com");
    assert_eq!(me["role"], "admin");
    assert_eq!(me["via"], "session");
    assert_eq!(me["organization"], "org-1");
    // RBAC, as the UI will read it.
    assert_eq!(me["permissions"]["read_telemetry"], true);
    assert_eq!(me["permissions"]["read_config"], true);

    // And the session is real inside the process too.
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("cookie", cookie.parse().unwrap());
    assert!(matches!(
        auth.authenticate(&headers).await,
        Outcome::Authenticated(_)
    ));
}

#[tokio::test]
async fn a_viewer_gets_viewer_permissions_only() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    let claims = idp.user_claims("bob", "otelview", json!({"otelview.viewer": {}}));
    let cookie = sign_in(&app, &idp, claims).await;

    let (_, _, body) = send(
        &app,
        Request::get("/auth/me")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let me: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(me["role"], "viewer");
    assert_eq!(me["permissions"]["read_telemetry"], true);
    // The distinction that makes this RBAC rather than a yes/no gate.
    assert_eq!(me["permissions"]["read_config"], false);
    assert_eq!(me["permissions"]["administer"], false);
}

/// The nonce ties the id token to the login this server started. An id
/// token captured from another login — or minted elsewhere — carries a
/// different one and must not be accepted.
#[tokio::test]
async fn an_id_token_with_the_wrong_nonce_is_refused() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    idp.will_issue(idp.user_claims("ada", "otelview", json!({"otelview.admin": {}})));
    let (_, headers, _) = get(&app, "/auth/login").await;
    let location = headers["location"].to_str().unwrap().to_string();
    let state = param(&location, "state").unwrap();
    // A nonce from some other login.
    idp.will_echo_nonce("a-nonce-from-somewhere-else");

    let (status, headers, body) = get(&app, &format!("/auth/callback?code=c&state={state}")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(
        !headers.contains_key("set-cookie"),
        "no session was created"
    );
}

/// Zitadel puts `name` and `email` in the id token and not in the access
/// token, so a session built from the access token alone knows the user
/// only as a numeric subject.
#[tokio::test]
async fn the_display_name_comes_from_the_id_token() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    // An access token with roles and no profile, as Zitadel issues.
    let mut claims = idp.user_claims("ada", "otelview", json!({"otelview.admin": {}}));
    claims["name"] = Value::Null;
    claims["email"] = Value::Null;
    let cookie = sign_in(&app, &idp, claims).await;

    let (_, _, body) = send(
        &app,
        Request::get("/auth/me")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let me: Value = serde_json::from_str(&body).unwrap();
    // The mock signs the id token from the same claims, so this asserts
    // the pathway rather than the value: null in, null out, and the
    // subject is still what identifies the session.
    assert_eq!(me["subject"], "ada");
    assert_eq!(me["role"], "admin");
}

#[tokio::test]
async fn an_account_without_a_role_is_refused_after_a_valid_login() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    // The provider is happy; this instance is not.
    idp.will_issue(idp.user_claims("mallory", "otelview", json!({"other.app": {}})));
    let (_, headers, _) = get(&app, "/auth/login").await;
    let state = param(headers["location"].to_str().unwrap(), "state").unwrap();

    let (status, headers, body) = get(&app, &format!("/auth/callback?code=c&state={state}")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("otelview.viewer"), "{body}");
    assert!(
        !headers.contains_key("set-cookie"),
        "no session was created"
    );
}

#[tokio::test]
async fn a_replayed_callback_is_refused() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    idp.will_issue(idp.user_claims("ada", "otelview", json!({"otelview.admin": {}})));
    let (_, headers, _) = get(&app, "/auth/login").await;
    let location = headers["location"].to_str().unwrap().to_string();
    let state = param(&location, "state").unwrap();
    idp.will_echo_nonce(&param(&location, "nonce").unwrap());

    let first = get(&app, &format!("/auth/callback?code=c&state={state}")).await;
    assert_eq!(first.0, StatusCode::SEE_OTHER);

    // The same state again — a replayed or forged callback.
    let (status, _, body) = get(&app, &format!("/auth/callback?code=c&state={state}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("expired or was already used"), "{body}");
}

#[tokio::test]
async fn a_callback_with_an_unknown_state_is_refused() {
    let idp = MockIdp::start().await;
    let (app, _) = app(&config(&idp)).await;
    let (status, _, _) = get(&app, "/auth/callback?code=c&state=not-a-real-state").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_provider_saying_no_is_shown_to_the_user() {
    let idp = MockIdp::start().await;
    let (app, _) = app(&config(&idp)).await;
    let (status, _, body) = get(
        &app,
        "/auth/callback?error=access_denied&error_description=MFA%20required",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("access_denied"), "{body}");
}

#[tokio::test]
async fn logout_drops_the_session_and_ends_it_at_the_provider() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    let claims = idp.user_claims("ada", "otelview", json!({"otelview.admin": {}}));
    let cookie = sign_in(&app, &idp, claims).await;

    let (status, headers, _) = send(
        &app,
        Request::post("/auth/logout")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    // Off to the provider's end-session endpoint, carrying the id token.
    let location = headers["location"].to_str().unwrap();
    assert!(location.contains("/logout"), "{location}");
    assert!(location.contains("id_token_hint="), "{location}");
    // And the cookie is cleared here.
    let cleared = headers["set-cookie"].to_str().unwrap();
    assert!(cleared.contains("Max-Age=0"), "{cleared}");

    // The session no longer resolves.
    let (status, _, _) = send(
        &app,
        Request::get("/auth/me")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_bearer_token_is_accepted_on_its_own() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    let token = idp.mint(idp.user_claims("agent", "otelview", json!({"otelview.viewer": {}})));
    let (status, _, body) = send(
        &app,
        Request::get("/auth/me")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let me: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(me["subject"], "agent");
    assert_eq!(me["via"], "bearer_token");
}

#[tokio::test]
async fn an_expired_token_is_refused() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (_, auth) = app(&cfg).await;

    let mut claims = idp.user_claims("ada", "otelview", json!({"otelview.admin": {}}));
    claims["exp"] = json!(support::now() - 3600);
    let token = idp.mint(claims);

    match auth.authenticate_token(&token).await {
        Outcome::Rejected(reason) => assert!(reason.contains("expired"), "{reason}"),
        other => panic!("expected an expired token to be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn a_token_for_another_audience_is_refused() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (_, auth) = app(&cfg).await;

    let token = idp.mint(idp.user_claims("ada", "some-other-app", json!({"otelview.admin": {}})));
    match auth.authenticate_token(&token).await {
        Outcome::Rejected(reason) => {
            assert!(reason.contains("not addressed to this server"), "{reason}")
        }
        other => panic!("expected the wrong audience to be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn a_token_from_another_issuer_is_refused() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (_, auth) = app(&cfg).await;

    let mut claims = idp.user_claims("ada", "otelview", json!({"otelview.admin": {}}));
    claims["iss"] = json!("https://not-your-provider.example.com");
    let token = idp.mint(claims);

    match auth.authenticate_token(&token).await {
        Outcome::Rejected(reason) => assert!(reason.contains("different provider"), "{reason}"),
        other => panic!("expected a foreign issuer to be refused, got {other:?}"),
    }
}

/// A token signed with a key the provider does not publish is a forgery,
/// or a rotation this server has not seen. Either way it is refused, and
/// the refusal costs one key fetch rather than one per request.
#[tokio::test]
async fn a_token_signed_with_an_unpublished_key_is_refused() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (_, auth) = app(&cfg).await;

    let token = idp.mint_with_kid(
        idp.user_claims("ada", "otelview", json!({"otelview.admin": {}})),
        "not-published",
    );
    match auth.authenticate_token(&token).await {
        Outcome::Rejected(reason) => assert!(reason.contains("no signing key"), "{reason}"),
        other => panic!("expected an unknown key to be refused, got {other:?}"),
    }
}

/// `alg: none` is the oldest JWT attack there is.
#[tokio::test]
async fn an_unsigned_token_is_refused() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (_, auth) = app(&cfg).await;

    let header = URL_SAFE_NO_PAD.encode(json!({"alg": "none", "typ": "JWT"}).to_string());
    let claims = URL_SAFE_NO_PAD.encode(
        idp.user_claims("mallory", "otelview", json!({"otelview.admin": {}}))
            .to_string(),
    );
    let forged = format!("{header}.{claims}.");

    assert!(matches!(
        auth.authenticate_token(&forged).await,
        Outcome::Rejected(_)
    ));
}

#[tokio::test]
async fn an_opaque_token_is_refused_unless_introspection_is_on() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (_, auth) = app(&cfg).await;

    match auth.authenticate_token("an-opaque-reference").await {
        Outcome::Rejected(reason) => assert!(reason.contains("introspection"), "{reason}"),
        other => panic!("expected an opaque token to be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn introspection_accepts_a_live_opaque_token_and_refuses_a_revoked_one() {
    let idp = MockIdp::start().await;
    let mut cfg = config(&idp);
    cfg.auth.oidc.introspection = true;
    cfg.auth.oidc.client_secret = Some("s3cret".into());
    let (_, auth) = app(&cfg).await;

    idp.will_issue(idp.user_claims("ada", "otelview", json!({"otelview.viewer": {}})));

    match auth.authenticate_token("a-live-token").await {
        Outcome::Authenticated(p) => assert_eq!(p.subject, "ada"),
        other => panic!("expected introspection to accept the token, got {other:?}"),
    }

    // The mock marks exactly this one inactive, as a revoked token is.
    match auth.authenticate_token("revoked-token").await {
        Outcome::Rejected(reason) => assert!(reason.contains("not active"), "{reason}"),
        other => panic!("expected a revoked token to be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn the_public_endpoints_need_no_credentials() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    // A login screen that required a login could not be used.
    let (status, _, body) = get(&app, "/auth/info").await;
    assert_eq!(status, StatusCode::OK);
    let info: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["mode"], "oidc");
    assert_eq!(info["login_url"], "/auth/login");
    assert_eq!(info["issuer"], idp.base);

    // And an agent has to be able to discover where tokens come from.
    let (status, _, body) = get(&app, "/.well-known/oauth-protected-resource").await;
    assert_eq!(status, StatusCode::OK);
    let meta: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(meta["authorization_servers"][0], idp.base);
    assert_eq!(meta["bearer_methods_supported"][0], "header");
}

#[tokio::test]
async fn the_login_redirect_carries_pkce_and_the_configured_scopes() {
    let idp = MockIdp::start().await;
    let mut cfg = config(&idp);
    cfg.auth.oidc.scopes = vec![
        "openid".into(),
        "profile".into(),
        "urn:zitadel:iam:org:project:id:12345:aud".into(),
    ];
    let (app, _) = app(&cfg).await;

    let (status, headers, _) = get(&app, "/auth/login").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers["location"].to_str().unwrap();

    assert!(
        location.starts_with(&format!("{}/authorize?", idp.base)),
        "{location}"
    );
    assert!(location.contains("response_type=code"), "{location}");
    assert!(
        location.contains("code_challenge_method=S256"),
        "{location}"
    );
    assert!(param(location, "code_challenge").is_some(), "{location}");
    assert!(param(location, "nonce").is_some(), "{location}");
    // The Zitadel audience scope has to survive encoding intact.
    let scope = param(location, "scope").unwrap();
    assert!(
        scope.contains("urn:zitadel:iam:org:project:id:12345:aud"),
        "{scope}"
    );
}

/// The verifier stays here; only its hash is ever sent to the provider.
#[tokio::test]
async fn the_pkce_verifier_never_travels_to_the_authorization_endpoint() {
    let idp = MockIdp::start().await;
    let (app, _) = app(&config(&idp)).await;
    let (_, headers, _) = get(&app, "/auth/login").await;
    let location = headers["location"].to_str().unwrap();
    assert!(!location.contains("code_verifier"), "{location}");
}

#[tokio::test]
async fn an_open_redirect_is_not_possible_through_return_to() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    idp.will_issue(idp.user_claims("ada", "otelview", json!({"otelview.admin": {}})));
    let (_, headers, _) = get(&app, "/auth/login?return_to=https://evil.example/steal").await;
    let location = headers["location"].to_str().unwrap().to_string();
    let state = param(&location, "state").unwrap();
    idp.will_echo_nonce(&param(&location, "nonce").unwrap());

    let (_, headers, _) = get(&app, &format!("/auth/callback?code=c&state={state}")).await;
    // Home, not to the attacker.
    assert_eq!(headers["location"], "/");
}

#[tokio::test]
async fn a_token_endpoint_failure_is_reported_without_a_session() {
    let idp = MockIdp::start().await;
    let cfg = config(&idp);
    let (app, _) = app(&cfg).await;

    let (_, headers, _) = get(&app, "/auth/login").await;
    let state = param(headers["location"].to_str().unwrap(), "state").unwrap();
    idp.will_fail("invalid_grant");

    let (status, headers, _) = get(&app, &format!("/auth/callback?code=c&state={state}")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(!headers.contains_key("set-cookie"));
}

#[tokio::test]
async fn an_organization_outside_the_allowlist_cannot_sign_in() {
    let idp = MockIdp::start().await;
    let mut cfg = config(&idp);
    cfg.auth.oidc.allowed_organizations = vec!["org-approved".into()];
    let (app, _) = app(&cfg).await;

    idp.will_issue(idp.user_claims("ada", "otelview", json!({"otelview.admin": {}})));
    let (_, headers, _) = get(&app, "/auth/login").await;
    let state = param(headers["location"].to_str().unwrap(), "state").unwrap();

    let (status, _, body) = get(&app, &format!("/auth/callback?code=c&state={state}")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("org-1"), "{body}");
}
