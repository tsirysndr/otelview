//! The identity provider, as seen from here: its metadata and its keys.
//!
//! Both are fetched over the network and cached. Discovery is read once —
//! endpoints do not move while a process runs — while the signing keys are
//! re-read on an interval, because they rotate and a server that cached
//! them forever would start rejecting perfectly good tokens some morning
//! with no deploy to blame.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use jsonwebtoken::jwk::{Jwk, JwkSet};
use serde::Deserialize;
use tokio::sync::RwLock;

/// The subset of an OpenID Provider's metadata this server uses.
///
/// Deliberately not `deny_unknown_fields`: providers add fields, and a new
/// one in Zitadel's document is not a reason to stop logging people in.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
    pub jwks_uri: String,
    #[serde(default)]
    pub introspection_endpoint: Option<String>,
    #[serde(default)]
    pub end_session_endpoint: Option<String>,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
}

impl ProviderMetadata {
    /// True when the provider advertises PKCE with SHA-256.
    ///
    /// Absent metadata is treated as support: the field is optional in the
    /// spec, plenty of providers omit it while implementing PKCE, and the
    /// authorization request fails loudly if it turns out not to.
    pub fn supports_pkce_s256(&self) -> bool {
        self.code_challenge_methods_supported.is_empty()
            || self
                .code_challenge_methods_supported
                .iter()
                .any(|m| m == "S256")
    }
}

/// Cached provider metadata and signing keys.
pub struct Provider {
    http: reqwest::Client,
    discovery_url: String,
    /// The issuer this server was configured with, checked against the one
    /// the document claims.
    expected_issuer: String,
    metadata: RwLock<Option<Arc<ProviderMetadata>>>,
    keys: RwLock<KeyCache>,
    refresh_interval: Duration,
}

#[derive(Default)]
struct KeyCache {
    set: Option<JwkSet>,
    fetched_at: Option<Instant>,
}

impl KeyCache {
    fn stale(&self, interval: Duration) -> bool {
        match self.fetched_at {
            None => true,
            Some(at) => at.elapsed() >= interval,
        }
    }
}

/// How long any single call to the provider may take.
const TIMEOUT: Duration = Duration::from_secs(15);

impl Provider {
    pub fn new(
        discovery_url: String,
        expected_issuer: String,
        refresh_interval: Duration,
    ) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .user_agent(concat!("otelview/", env!("CARGO_PKG_VERSION")))
                .build()
                .context("building the HTTP client for the identity provider")?,
            discovery_url,
            expected_issuer: expected_issuer.trim_end_matches('/').to_string(),
            metadata: RwLock::new(None),
            keys: RwLock::new(KeyCache::default()),
            refresh_interval,
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// The provider's metadata, fetched once and then remembered.
    pub async fn metadata(&self) -> Result<Arc<ProviderMetadata>> {
        if let Some(m) = self.metadata.read().await.clone() {
            return Ok(m);
        }
        let mut slot = self.metadata.write().await;
        // Another task may have filled it while this one waited.
        if let Some(m) = slot.clone() {
            return Ok(m);
        }
        let fetched = self.fetch_metadata().await?;
        let fetched = Arc::new(fetched);
        *slot = Some(fetched.clone());
        Ok(fetched)
    }

    async fn fetch_metadata(&self) -> Result<ProviderMetadata> {
        tracing::debug!(url = %self.discovery_url, "reading OIDC discovery document");
        let resp = self
            .http
            .get(&self.discovery_url)
            .send()
            .await
            .with_context(|| format!("GET {}", self.discovery_url))?;
        let status = resp.status();
        if !status.is_success() {
            bail!(
                "the identity provider answered {status} at {}; is auth.oidc.issuer right?",
                self.discovery_url
            );
        }
        let metadata: ProviderMetadata = resp
            .json()
            .await
            .with_context(|| format!("parsing the discovery document at {}", self.discovery_url))?;

        // The issuer in the document is what tokens will carry. If it
        // disagrees with the configured one, every token would fail its
        // issuer check later with a much more confusing message.
        if metadata.issuer.trim_end_matches('/') != self.expected_issuer {
            bail!(
                "the provider at {} calls itself {:?}, but auth.oidc.issuer is {:?}; \
                 tokens carry the former, so they must match",
                self.discovery_url,
                metadata.issuer,
                self.expected_issuer
            );
        }
        tracing::info!(
            issuer = %metadata.issuer,
            pkce = metadata.supports_pkce_s256(),
            introspection = metadata.introspection_endpoint.is_some(),
            "identity provider discovered"
        );
        Ok(metadata)
    }

    /// The signing key for `kid`.
    ///
    /// A key id that is not in the cache forces one refresh: that is what
    /// rotation looks like from here. The refresh is rate-limited by the
    /// configured interval so that tokens signed with a genuinely unknown
    /// key — a different provider, a forgery — cannot turn into a request
    /// per token against the provider.
    pub async fn signing_key(&self, kid: &str) -> Result<Jwk> {
        if let Some(key) = self.cached_key(kid).await {
            return Ok(key);
        }
        self.refresh_keys(false).await?;
        self.cached_key(kid)
            .await
            .with_context(|| format!("no signing key {kid:?} at the provider's jwks_uri"))
    }

    async fn cached_key(&self, kid: &str) -> Option<Jwk> {
        let cache = self.keys.read().await;
        if cache.stale(self.refresh_interval) {
            return None;
        }
        cache.set.as_ref()?.find(kid).cloned()
    }

    /// Re-read the key set. `force` ignores the rate limit.
    pub async fn refresh_keys(&self, force: bool) -> Result<()> {
        let mut cache = self.keys.write().await;
        if !force && !cache.stale(self.refresh_interval) {
            return Ok(());
        }
        let jwks_uri = self.metadata().await?.jwks_uri.clone();
        tracing::debug!(url = %jwks_uri, "fetching signing keys");
        let resp = self
            .http
            .get(&jwks_uri)
            .send()
            .await
            .with_context(|| format!("GET {jwks_uri}"))?;
        if !resp.status().is_success() {
            bail!("the provider answered {} at {jwks_uri}", resp.status());
        }
        let set: JwkSet = resp
            .json()
            .await
            .with_context(|| format!("parsing the key set at {jwks_uri}"))?;
        tracing::debug!(keys = set.keys.len(), "signing keys cached");
        cache.set = Some(set);
        cache.fetched_at = Some(Instant::now());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(methods: &[&str]) -> ProviderMetadata {
        ProviderMetadata {
            issuer: "https://auth.example.com".into(),
            authorization_endpoint: "https://auth.example.com/authorize".into(),
            token_endpoint: "https://auth.example.com/token".into(),
            userinfo_endpoint: None,
            jwks_uri: "https://auth.example.com/keys".into(),
            introspection_endpoint: None,
            end_session_endpoint: None,
            revocation_endpoint: None,
            scopes_supported: Vec::new(),
            code_challenge_methods_supported: methods.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn pkce_support_is_assumed_when_unstated() {
        assert!(metadata(&[]).supports_pkce_s256());
        assert!(metadata(&["plain", "S256"]).supports_pkce_s256());
        assert!(!metadata(&["plain"]).supports_pkce_s256());
    }

    #[test]
    fn a_key_cache_past_its_interval_is_stale() {
        let fresh = KeyCache {
            set: None,
            fetched_at: Some(Instant::now()),
        };
        assert!(!fresh.stale(Duration::from_secs(60)));
        assert!(fresh.stale(Duration::from_millis(0)));
        assert!(KeyCache::default().stale(Duration::from_secs(60)));
    }

    /// Unknown fields are the normal state of a discovery document.
    #[test]
    fn metadata_parses_past_fields_it_does_not_know() {
        let doc = serde_json::json!({
            "issuer": "https://auth.example.com",
            "authorization_endpoint": "https://auth.example.com/oauth/v2/authorize",
            "token_endpoint": "https://auth.example.com/oauth/v2/token",
            "jwks_uri": "https://auth.example.com/oauth/v2/keys",
            "introspection_endpoint": "https://auth.example.com/oauth/v2/introspect",
            "end_session_endpoint": "https://auth.example.com/oidc/v1/end_session",
            "code_challenge_methods_supported": ["S256"],
            "request_object_signing_alg_values_supported": ["RS256"],
            "something_new_in_the_next_release": true
        });
        let m: ProviderMetadata = serde_json::from_value(doc).unwrap();
        assert_eq!(m.issuer, "https://auth.example.com");
        assert!(m.supports_pkce_s256());
        assert!(m.introspection_endpoint.is_some());
        assert!(m.userinfo_endpoint.is_none());
    }
}
