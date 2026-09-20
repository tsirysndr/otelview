//! Configuration for otelview, loadable from YAML or TOML.
//!
//! Every field has a default so an empty file (or no file) yields a working
//! instance: OTLP gRPC on :4317, OTLP HTTP on :4318, UI/API on :4319,
//! in-memory storage.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub receivers: Receivers,
    pub auth: Auth,
    pub storage: StorageConfig,
    pub ui: UiConfig,
    pub log_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Receivers {
    pub grpc: Endpoint,
    pub http: Endpoint,
}

impl Default for Receivers {
    fn default() -> Self {
        Self {
            grpc: Endpoint {
                enabled: true,
                listen: "0.0.0.0:4317".into(),
            },
            http: Endpoint {
                enabled: true,
                listen: "0.0.0.0:4318".into(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Endpoint {
    pub enabled: bool,
    pub listen: String,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self {
            enabled: true,
            listen: String::new(),
        }
    }
}

/// Optional header authentication for the OTLP receivers.
///
/// When `token` is set, every ingest request must carry `header: <token>`
/// (gRPC metadata key or HTTP header). The query API is guarded by the same
/// token only if `protect_api` is true.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Auth {
    pub header: String,
    pub token: Option<String>,
    pub protect_api: bool,
}

impl Default for Auth {
    fn default() -> Self {
        Self {
            header: "x-otelview-token".into(),
            token: None,
            protect_api: false,
        }
    }
}

impl Auth {
    pub fn enabled(&self) -> bool {
        self.token
            .as_deref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Memory,
    Duckdb,
    Jaeger,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct StorageConfig {
    pub backend: Backend,
    pub memory: MemoryConfig,
    pub duckdb: DuckdbConfig,
    pub jaeger: JaegerConfig,
    pub remote: RemoteConfig,
    /// How long telemetry lives before a background sweep deletes it:
    /// "36h", "7d", "2w", "1mo" (months count as 30 days). Unset keeps
    /// everything forever. Honoured by backends that own their data —
    /// duckdb; the remote postgres storage has its own RETENTION knob.
    pub retention: Option<String>,
    /// How often the retention sweep runs. Same format as `retention`.
    pub retention_sweep_interval: String,
}

/// Parse a humane duration: `<n><unit>` with unit `h`, `d`, `w`, `m`/`mo`
/// (months, as 30 days). Whitespace and a trailing `s` are tolerated so
/// "2 weeks" works. The same grammar as the postgres storage's RETENTION.
pub fn parse_duration(raw: &str) -> Result<std::time::Duration> {
    let s = raw.trim().to_lowercase().replace(' ', "");
    let split = s
        .find(|c: char| !c.is_ascii_digit())
        .with_context(|| format!("duration {raw:?} has no unit (try 7d, 2w, 1mo)"))?;
    let (digits, unit) = s.split_at(split);
    let n: u64 = digits
        .parse()
        .with_context(|| format!("duration {raw:?} has no number (try 7d, 2w, 1mo)"))?;
    if n == 0 {
        bail!("duration {raw:?} is zero, which would delete everything on arrival");
    }
    let hours = match unit.trim_end_matches('s') {
        "h" | "hour" => n,
        "d" | "day" => n * 24,
        "w" | "week" => n * 24 * 7,
        "m" | "mo" | "month" => n * 24 * 30,
        other => bail!("duration unit {other:?} not understood (h, d, w, m/mo)"),
    };
    Ok(std::time::Duration::from_secs(hours * 3600))
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Memory,
            memory: MemoryConfig::default(),
            duckdb: DuckdbConfig::default(),
            jaeger: JaegerConfig::default(),
            remote: RemoteConfig::default(),
            retention: None,
            retention_sweep_interval: "1h".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MemoryConfig {
    pub max_spans: usize,
    pub max_logs: usize,
    pub max_metric_points: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_spans: 200_000,
            max_logs: 200_000,
            max_metric_points: 500_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DuckdbConfig {
    /// Filesystem path of the database, or ":memory:".
    pub path: String,
}

impl Default for DuckdbConfig {
    fn default() -> Self {
        Self {
            path: "otelview.duckdb".into(),
        }
    }
}

/// External trace storage speaking the Jaeger v2 remote-storage gRPC API
/// (`jaeger.storage.v2.TraceReader` for reads, OTLP `TraceService/Export`
/// for writes). Logs and metrics are not part of that API, so they are kept
/// in the local `fallback` backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct JaegerConfig {
    /// gRPC endpoint, e.g. "http://127.0.0.1:17271".
    pub endpoint: String,
    /// Local backend for logs and metrics: memory or duckdb.
    pub fallback: FallbackBackend,
}

impl Default for JaegerConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            fallback: FallbackBackend::Memory,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackBackend {
    Memory,
    Duckdb,
}

/// Full remote storage: another otelview instance (or any backend serving
/// `jaeger.storage.v2.TraceReader` + `otelview.storage.v1.{LogReader,
/// MetricReader}` + the OTLP collector Export services) at one gRPC endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RemoteConfig {
    /// gRPC endpoint, e.g. "http://other-host:4317".
    pub endpoint: String,
    /// Optional auth header/token forwarded to the remote instance.
    pub auth_header: String,
    pub auth_token: Option<String>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            auth_header: "x-otelview-token".into(),
            auth_token: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiConfig {
    pub listen: String,
    /// Allow cross-origin API access (useful for the Tauri desktop app).
    pub cors: bool,
    /// Optional token required to use the web UI (sent as a Bearer token or
    /// via the auth header). Unset = UI open.
    pub token: Option<String>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:4319".into(),
            cors: true,
            token: None,
        }
    }
}

impl UiConfig {
    pub fn auth_enabled(&self) -> bool {
        self.token
            .as_deref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let config: Config = match ext {
            "yaml" | "yml" => serde_yaml::from_str(&raw).context("parsing YAML config")?,
            "toml" => toml::from_str(&raw).context("parsing TOML config")?,
            _ => {
                // No/unknown extension: try YAML first (superset-ish for our
                // shapes), then TOML, and report both errors on failure.
                match serde_yaml::from_str(&raw) {
                    Ok(c) => c,
                    Err(yaml_err) => match toml::from_str(&raw) {
                        Ok(c) => c,
                        Err(toml_err) => bail!(
                            "config is neither valid YAML ({yaml_err}) nor valid TOML ({toml_err})"
                        ),
                    },
                }
            }
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        // Refused at startup rather than ignored: a typo silently keeping
        // data forever is the exact failure a retention knob exists to
        // prevent.
        if let Some(retention) = self
            .storage
            .retention
            .as_deref()
            .filter(|r| !r.trim().is_empty())
        {
            parse_duration(retention).context("storage.retention")?;
            parse_duration(&self.storage.retention_sweep_interval)
                .context("storage.retention_sweep_interval")?;
        }
        if self.storage.backend == Backend::Jaeger && self.storage.jaeger.endpoint.is_empty() {
            bail!("storage.backend is 'jaeger' but storage.jaeger.endpoint is empty");
        }
        if self.storage.backend == Backend::Remote && self.storage.remote.endpoint.is_empty() {
            bail!("storage.backend is 'remote' but storage.remote.endpoint is empty");
        }
        if self.receivers.grpc.enabled && self.receivers.grpc.listen.is_empty() {
            bail!("receivers.grpc.listen must be set when enabled");
        }
        if self.receivers.http.enabled && self.receivers.http.listen.is_empty() {
            bail!("receivers.http.listen must be set when enabled");
        }
        Ok(())
    }

    /// Copy with secrets blanked, safe to expose over the API.
    pub fn sanitized(&self) -> Self {
        let mut c = self.clone();
        if c.auth.token.is_some() {
            c.auth.token = Some("***".into());
        }
        if c.storage.remote.auth_token.is_some() {
            c.storage.remote.auth_token = Some("***".into());
        }
        if c.ui.token.is_some() {
            c.ui.token = Some("***".into());
        }

        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn parses_yaml() {
        let c: Config = serde_yaml::from_str(
            r#"
receivers:
  grpc: { listen: "0.0.0.0:14317" }
auth:
  token: sekret
storage:
  backend: duckdb
  duckdb: { path: "/tmp/x.duckdb" }
"#,
        )
        .unwrap();
        assert_eq!(c.receivers.grpc.listen, "0.0.0.0:14317");
        assert!(c.auth.enabled());
        assert_eq!(c.storage.backend, Backend::Duckdb);
    }

    #[test]
    fn parses_toml() {
        let c: Config = toml::from_str(
            r#"
[storage]
backend = "jaeger"
[storage.jaeger]
endpoint = "http://localhost:17271"
[ui]
listen = "0.0.0.0:8080"
"#,
        )
        .unwrap();
        assert_eq!(c.storage.backend, Backend::Jaeger);
        assert_eq!(c.ui.listen, "0.0.0.0:8080");
        c.validate().unwrap();
    }

    #[test]
    fn humane_retention_durations_parse() {
        let hours = |s: &str| parse_duration(s).unwrap().as_secs() / 3600;
        assert_eq!(hours("36h"), 36);
        assert_eq!(hours("7d"), 7 * 24);
        assert_eq!(hours("2 weeks"), 14 * 24);
        assert_eq!(hours("1mo"), 30 * 24);
        assert_eq!(hours("1m"), 30 * 24, "m is months, not minutes");
    }

    #[test]
    fn a_retention_typo_refuses_the_config() {
        let mut c = Config::default();
        c.storage.retention = Some("7 fortnights".into());
        assert!(c.validate().is_err());

        c.storage.retention = Some("7d".into());
        c.storage.retention_sweep_interval = "soon".into();
        assert!(c.validate().is_err());

        c.storage.retention_sweep_interval = "1h".into();
        c.validate().unwrap();
    }
}
