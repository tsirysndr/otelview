//! A mock OpenID provider, so the flow can be tested end to end.
//!
//! Behind the `testing` feature and shipped in the library rather than in
//! this crate's own `tests/`, so that the api and mcp crates can point
//! their own integration tests at a provider that really signs tokens
//! instead of each inventing a weaker one.
//!
//! It serves a real discovery document, a real JWKS, and a token endpoint
//! that mints real RS256 tokens — so the code under test does exactly what
//! it does against Zitadel: fetch metadata, fetch keys, verify a
//! signature, check `iss`, `aud`, `exp` and `nbf`. Nothing here is
//! stubbed out inside the crate; the only thing pretending is the
//! provider at the other end of the socket.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::{json, Value};

mod key;
pub use key::*;

/// What the provider will put in the next token it issues.
#[derive(Clone)]
pub struct TokenRecipe {
    pub claims: Value,
    /// Sign with a key id the JWKS does not publish, to test rotation and
    /// forgery paths.
    pub kid: String,
    /// Hand back an opaque token rather than a JWT.
    pub opaque: Option<String>,
    /// The nonce to echo in the id token, as a real provider echoes the
    /// one it was given in the authorization request.
    pub nonce: Option<String>,
    /// Claims that appear in the id token only. Real providers put the
    /// profile here and not in the access token, so tests can only tell
    /// the two apart if the mock can too.
    pub id_only: Value,
    /// Answer the token endpoint with an error instead.
    pub fail: Option<String>,
}

pub struct MockIdp {
    pub base: String,
    recipe: Arc<Mutex<TokenRecipe>>,
    /// Every form posted to the token endpoint, so a test can assert the
    /// PKCE verifier actually travelled.
    pub token_requests: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
struct IdpState {
    base: String,
    recipe: Arc<Mutex<TokenRecipe>>,
    token_requests: Arc<Mutex<Vec<String>>>,
    introspection: bool,
}

impl MockIdp {
    pub async fn start() -> Self {
        Self::start_with(true).await
    }

    pub async fn start_with(introspection: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let base = format!("http://127.0.0.1:{}", addr.port());

        let recipe = Arc::new(Mutex::new(TokenRecipe {
            claims: json!({}),
            kid: TEST_KID.to_string(),
            opaque: None,
            nonce: None,
            id_only: json!({}),
            fail: None,
        }));
        let token_requests = Arc::new(Mutex::new(Vec::new()));

        let state = IdpState {
            base: base.clone(),
            recipe: recipe.clone(),
            token_requests: token_requests.clone(),
            introspection,
        };
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/keys", get(jwks))
            .route("/token", post(token))
            .route("/introspect", post(introspect))
            .with_state(state);

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            base,
            recipe,
            token_requests,
        }
    }

    /// Decide what the next issued token looks like.
    pub fn will_issue(&self, claims: Value) {
        let mut recipe = self.recipe.lock().unwrap();
        recipe.claims = claims;
        recipe.opaque = None;
        recipe.fail = None;
        recipe.kid = TEST_KID.to_string();
    }

    /// Claims for an access token shaped the way Zitadel shapes one: the
    /// roles and the audience, and no profile at all.
    pub fn roles_only_claims(&self, sub: &str, audience: &str, roles: Value) -> Value {
        let mut claims = self.user_claims(sub, audience, roles);
        let obj = claims.as_object_mut().expect("claims are an object");
        obj.remove("name");
        obj.remove("email");
        claims
    }

    /// Echo this nonce in the next id token. A provider does this with
    /// whatever arrived in the authorization request; the tests pull it
    /// out of the redirect and hand it back the same way.
    pub fn will_echo_nonce(&self, nonce: &str) {
        self.recipe.lock().unwrap().nonce = Some(nonce.to_string());
    }

    /// Put these claims in the id token and nowhere else.
    pub fn will_issue_profile(&self, claims: Value) {
        self.recipe.lock().unwrap().id_only = claims;
    }

    pub fn will_issue_opaque(&self, token: &str) {
        let mut recipe = self.recipe.lock().unwrap();
        recipe.opaque = Some(token.to_string());
    }

    pub fn will_fail(&self, error: &str) {
        self.recipe.lock().unwrap().fail = Some(error.to_string());
    }

    pub fn will_sign_with_unknown_key(&self) {
        self.recipe.lock().unwrap().kid = "a-key-nobody-published".into();
    }

    /// Mint a token directly, for tests that skip the browser flow.
    pub fn mint(&self, claims: Value) -> String {
        sign(&claims, TEST_KID)
    }

    pub fn mint_with_kid(&self, claims: Value, kid: &str) -> String {
        sign(&claims, kid)
    }

    /// The claims a happy-path user carries.
    pub fn user_claims(&self, sub: &str, audience: &str, roles: Value) -> Value {
        json!({
            "iss": self.base,
            "sub": sub,
            "aud": audience,
            "exp": now() + 3600,
            "iat": now(),
            "nbf": now() - 10,
            "email": format!("{sub}@example.com"),
            "name": "Ada Lovelace",
            "urn:zitadel:iam:org:id": "org-1",
            "urn:zitadel:iam:org:project:roles": roles,
        })
    }

    pub fn token_request_count(&self) -> usize {
        self.token_requests.lock().unwrap().len()
    }

    pub fn last_token_request(&self) -> Option<String> {
        self.token_requests.lock().unwrap().last().cloned()
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn sign(claims: &Value, kid: &str) -> String {
    let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(kid.to_string());
    let key = EncodingKey::from_rsa_pem(TEST_KEY_PEM.as_bytes()).expect("the test key parses");
    encode(&header, claims, &key).expect("signing a test token")
}

async fn discovery(State(state): State<IdpState>) -> Json<Value> {
    let base = &state.base;
    let mut doc = json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "jwks_uri": format!("{base}/keys"),
        "end_session_endpoint": format!("{base}/logout"),
        "response_types_supported": ["code"],
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": ["openid", "profile", "email"],
    });
    if state.introspection {
        doc["introspection_endpoint"] = json!(format!("{base}/introspect"));
    }
    Json(doc)
}

async fn jwks() -> Json<Value> {
    Json(json!({
        "keys": [{
            "kty": "RSA",
            "use": "sig",
            "alg": "RS256",
            "kid": TEST_KID,
            "n": TEST_KEY_N,
            "e": TEST_KEY_E,
        }]
    }))
}

async fn token(State(state): State<IdpState>, body: String) -> Json<Value> {
    state.token_requests.lock().unwrap().push(body);
    let recipe = state.recipe.lock().unwrap().clone();
    if let Some(error) = recipe.fail {
        return Json(json!({"error": error}));
    }
    let access_token = match recipe.opaque {
        Some(opaque) => opaque,
        None => sign(&recipe.claims, &recipe.kid),
    };
    // The id token carries the nonce and the profile; the access token
    // carries neither, which is what Zitadel does and what the display
    // fields therefore have to come from.
    let mut id_claims = recipe.claims.clone();
    if let Some(nonce) = &recipe.nonce {
        id_claims["nonce"] = json!(nonce);
    }
    if let Some(extra) = recipe.id_only.as_object() {
        for (k, v) in extra {
            id_claims[k] = v.clone();
        }
    }
    Json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": 3600,
        "id_token": sign(&id_claims, &recipe.kid),
        "scope": "openid profile email",
    }))
}

async fn introspect(State(state): State<IdpState>, body: String) -> Json<Value> {
    // Whatever token is presented, answer with the current recipe's
    // claims — the tests decide whether it should look active.
    let recipe = state.recipe.lock().unwrap().clone();
    let presented = body
        .split('&')
        .find_map(|p| p.strip_prefix("token="))
        .unwrap_or_default()
        .to_string();
    let active = presented != "revoked-token";
    let mut claims = recipe.claims.clone();
    claims["active"] = json!(active);
    Json(claims)
}
