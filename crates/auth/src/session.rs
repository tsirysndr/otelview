//! Browser sessions.
//!
//! The cookie carries an opaque id and a signature over it, never the
//! tokens themselves: an access token in a cookie is an access token in
//! every proxy log and every `document.cookie`. The claims stay in this
//! process, keyed by that id.
//!
//! In memory, so a restart signs everyone out and a second replica does
//! not know the first one's sessions. That is a real constraint and the
//! deployment guide says so; it is also the honest default for a single
//! binary that deliberately has no external dependencies.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use rand::RngCore;
use sha2::Sha256;

use crate::principal::Principal;

type HmacSha256 = Hmac<Sha256>;

/// A signed-in browser.
#[derive(Debug, Clone)]
pub struct Session {
    pub principal: Principal,
    /// The access token from the provider, kept for calls made on the
    /// user's behalf and for logout. Never leaves this process.
    pub access_token: Option<String>,
    pub id_token: Option<String>,
    expires_at: Instant,
}

impl Session {
    pub fn expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// Sessions and the key their cookies are signed with.
pub struct SessionStore {
    sessions: Mutex<HashMap<String, Session>>,
    signing_key: [u8; 32],
    ttl: Duration,
    cookie_name: String,
    secure: bool,
}

/// Stop unbounded growth if something creates sessions in a loop. Well
/// above any real number of concurrent users for one binary.
const MAX_SESSIONS: usize = 10_000;

impl SessionStore {
    /// A store with a fresh random signing key.
    ///
    /// The key lives only in this process, which is the same statement as
    /// "a restart signs everyone out" — no key material on disk to leak,
    /// and no cookie from an older process silently still valid.
    pub fn new(ttl: Duration, cookie_name: String, secure: bool) -> Self {
        let mut signing_key = [0u8; 32];
        rand::rng().fill_bytes(&mut signing_key);
        Self {
            sessions: Mutex::new(HashMap::new()),
            signing_key,
            ttl,
            cookie_name,
            secure,
        }
    }

    pub fn cookie_name(&self) -> &str {
        &self.cookie_name
    }

    /// Store a session and return the cookie value for it.
    pub fn create(
        &self,
        principal: Principal,
        access_token: Option<String>,
        id_token: Option<String>,
    ) -> String {
        let id = random_id();
        let session = Session {
            principal,
            access_token,
            id_token,
            expires_at: Instant::now() + self.ttl,
        };
        {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            // Expiry is lazy, so a sweep here keeps a long-lived process
            // from holding every session it ever issued.
            sessions.retain(|_, s| !s.expired());
            if sessions.len() >= MAX_SESSIONS {
                tracing::warn!(
                    sessions = sessions.len(),
                    "session table is full; refusing to grow it further"
                );
                sessions.clear();
            }
            sessions.insert(id.clone(), session);
        }
        self.sign(&id)
    }

    /// The session a cookie value refers to, if the signature holds and it
    /// has not expired.
    pub fn get(&self, cookie_value: &str) -> Option<Session> {
        let id = self.verify(cookie_value)?;
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        match sessions.get(&id) {
            Some(s) if s.expired() => {
                sessions.remove(&id);
                None
            }
            Some(s) => Some(s.clone()),
            None => None,
        }
    }

    /// Forget a session. Returns what it held, so logout can tell the
    /// provider which id token is ending.
    pub fn remove(&self, cookie_value: &str) -> Option<Session> {
        let id = self.verify(cookie_value)?;
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
    }

    pub fn len(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `<id>.<mac>` — the id is opaque, the mac proves this server minted
    /// it. Without the mac a client could try session ids at will.
    fn sign(&self, id: &str) -> String {
        format!("{id}.{}", self.mac(id))
    }

    fn verify(&self, cookie_value: &str) -> Option<String> {
        let (id, mac) = cookie_value.rsplit_once('.')?;
        // Constant-time: a byte-by-byte comparison here leaks the
        // signature one character at a time.
        let expected = self.mac(id);
        constant_time_eq(mac.as_bytes(), expected.as_bytes()).then(|| id.to_string())
    }

    fn mac(&self, id: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.signing_key)
            .expect("HMAC accepts a key of any length");
        mac.update(id.as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }

    /// The `Set-Cookie` value for a session.
    ///
    /// `HttpOnly` keeps it away from scripts, `SameSite=Lax` keeps it off
    /// cross-site requests while still surviving the redirect back from
    /// the provider, and `Secure` is on unless a deployment has
    /// deliberately said it is testing over plain http.
    pub fn set_cookie(&self, value: &str) -> String {
        let mut cookie = format!(
            "{}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
            self.cookie_name,
            self.ttl.as_secs()
        );
        if self.secure {
            cookie.push_str("; Secure");
        }
        cookie
    }

    /// The `Set-Cookie` value that removes the session cookie.
    pub fn clear_cookie(&self) -> String {
        let mut cookie = format!(
            "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
            self.cookie_name
        );
        if self.secure {
            cookie.push_str("; Secure");
        }
        cookie
    }

    /// Read this store's cookie out of a `Cookie` header.
    pub fn from_headers(&self, headers: &axum::http::HeaderMap) -> Option<String> {
        let header = headers
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok())?;
        header.split(';').find_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            (name.trim() == self.cookie_name).then(|| value.trim().to_string())
        })
    }
}

/// 256 bits of randomness, url-safe.
pub fn random_id() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::{Credential, Role};

    fn store(ttl: Duration) -> SessionStore {
        SessionStore::new(ttl, "otelview_session".into(), true)
    }

    fn principal() -> Principal {
        Principal {
            subject: "user-1".into(),
            name: Some("Ada".into()),
            email: Some("ada@example.com".into()),
            organization: None,
            roles: vec!["otelview.admin".into()],
            role: Role::Admin,
            via: Credential::Session,
        }
    }

    #[test]
    fn a_session_round_trips_through_its_cookie() {
        let s = store(Duration::from_secs(60));
        let cookie = s.create(principal(), Some("at".into()), None);
        let got = s.get(&cookie).expect("the session this server just made");
        assert_eq!(got.principal.subject, "user-1");
        assert_eq!(got.access_token.as_deref(), Some("at"));
    }

    #[test]
    fn a_tampered_cookie_is_rejected() {
        let s = store(Duration::from_secs(60));
        let cookie = s.create(principal(), None, None);
        let (id, mac) = cookie.rsplit_once('.').unwrap();

        // Someone else's id with a valid-looking signature.
        assert!(s.get(&format!("{}.{mac}", random_id())).is_none());
        // The right id with a mangled signature.
        assert!(s.get(&format!("{id}.{}", "A".repeat(mac.len()))).is_none());
        // The id on its own, unsigned.
        assert!(s.get(id).is_none());
    }

    /// Each store signs with its own key, so a cookie from a previous
    /// process — or another instance — is not accepted here.
    #[test]
    fn cookies_do_not_cross_stores() {
        let a = store(Duration::from_secs(60));
        let b = store(Duration::from_secs(60));
        let cookie = a.create(principal(), None, None);
        assert!(a.get(&cookie).is_some());
        assert!(b.get(&cookie).is_none());
    }

    #[test]
    fn an_expired_session_is_gone_and_forgotten() {
        let s = store(Duration::from_millis(0));
        let cookie = s.create(principal(), None, None);
        assert!(s.get(&cookie).is_none());
        // Reading it also dropped it, rather than leaving it to sit.
        assert!(s.is_empty());
    }

    #[test]
    fn logout_removes_the_session() {
        let s = store(Duration::from_secs(60));
        let cookie = s.create(principal(), None, Some("id-token".into()));
        let removed = s.remove(&cookie).unwrap();
        assert_eq!(removed.id_token.as_deref(), Some("id-token"));
        assert!(s.get(&cookie).is_none());
        assert!(s.remove(&cookie).is_none());
    }

    #[test]
    fn the_cookie_is_httponly_lax_and_secure_when_asked() {
        let secure = store(Duration::from_secs(3600));
        let header = secure.set_cookie("abc");
        assert!(header.contains("HttpOnly"), "{header}");
        assert!(header.contains("SameSite=Lax"), "{header}");
        assert!(header.contains("Secure"), "{header}");
        assert!(header.contains("Max-Age=3600"), "{header}");

        let plain = SessionStore::new(Duration::from_secs(60), "s".into(), false);
        assert!(!plain.set_cookie("abc").contains("Secure"));
        assert!(plain.clear_cookie().contains("Max-Age=0"));
    }

    #[test]
    fn the_cookie_is_found_among_others() {
        let s = store(Duration::from_secs(60));
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "theme=dark; otelview_session=value-here; other=x"
                .parse()
                .unwrap(),
        );
        assert_eq!(s.from_headers(&headers).as_deref(), Some("value-here"));

        let mut none = axum::http::HeaderMap::new();
        none.insert(axum::http::header::COOKIE, "theme=dark".parse().unwrap());
        assert!(s.from_headers(&none).is_none());
        assert!(s.from_headers(&axum::http::HeaderMap::new()).is_none());
    }

    #[test]
    fn creating_a_session_sweeps_expired_ones() {
        let s = SessionStore::new(Duration::from_millis(0), "s".into(), false);
        for _ in 0..5 {
            s.create(principal(), None, None);
        }
        // Every one of those is already expired, so the newest create
        // leaves exactly itself behind.
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn random_ids_do_not_repeat() {
        let ids: std::collections::HashSet<String> = (0..100).map(|_| random_id()).collect();
        assert_eq!(ids.len(), 100);
    }
}
