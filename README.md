# otelview

[![ci](https://github.com/tsirysndr/otelview/actions/workflows/ci.yml/badge.svg)](https://github.com/tsirysndr/otelview/actions/workflows/ci.yml)

**The open-source, self-hosted OpenTelemetry viewer — traces, metrics and logs in one fast, beautiful, single binary.**

An alternative to Datadog, Kibana, CloudWatch and SigNoz you can run anywhere: one static binary embeds the OTLP receivers, the storage engine (DuckDB) and the web UI. No cluster, no JVM, no SaaS bill. Point your apps' OTLP exporters at it and open your browser.

```
┌─────────────────────────────── otelview (one binary) ───────────────────────────────┐
│                                                                                     │
│  OTLP gRPC :4317 ──┐                                       ┌── web UI + REST :4319  │
│  OTLP HTTP :4318 ──┼──► receivers ──► storage backend ◄────┤   (React, embedded)    │
│  (proto & JSON,    │    (optional     memory │ duckdb      └── remote-storage gRPC  │
│   gzip, header     │     header       jaeger │ remote          reader APIs on :4317 │
│   auth)            │     auth)                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

## Table of Contents

- [Highlights](#highlights)
- [Install](#install)
- [Quickstart](#quickstart)
- [Configuration](#configuration)
- [The remote-storage APIs](#the-remote-storage-apis)
- [Development](#development)
- [Releases](#releases)
- [License](#license)

## Highlights

- **All three signals**: trace search + waterfall, live log tail, metrics explorer with multi-series charts.
- **OTLP in, both transports**: gRPC (`:4317`) and HTTP (`:4318`), protobuf **and** JSON, gzip supported, optional header-token auth.
- **Storage your way**:
  - `memory` — bounded ring buffers, zero setup;
  - `duckdb` — embedded analytical store, persisted to a single file (linked **statically** from the official pre-built GitHub release binaries — DuckDB is never compiled from source);
  - `jaeger` — external trace storage via the **Jaeger v2 remote-storage gRPC API** (`jaeger.storage.v2.TraceReader` + OTLP export writes);
  - `remote` — another otelview instance as full storage for traces **and** logs **and** metrics.
- **It *is* a storage server too**: every otelview serves `jaeger.storage.v2.TraceReader` plus `otelview.storage.v1.{LogReader, MetricReader, Diagnostics}` (a logs/metrics read API modeled on the Jaeger v2 spec) on its gRPC port — so otelview can back Jaeger v2, and otelviews compose.
- **Single binary**: the React UI is embedded; the whole thing is one self-contained executable.
- **Desktop app**: a Tauri shell pointing at any remote otelview API.
- **Config**: YAML or TOML, every field optional, CLI overrides for the common knobs.

## Install

**Shell script** (macOS arm64, Linux x86_64/arm64):

```sh
curl -fsSL https://raw.githubusercontent.com/tsirysndr/otelview/main/install.sh | bash
```

**Docker**:

```sh
docker run -p 4317:4317 -p 4318:4318 -p 4319:4319 ghcr.io/tsirysndr/otelview
# persist DuckDB data:
docker run -p 4317:4317 -p 4318:4318 -p 4319:4319 \
  -v otelview-data:/data ghcr.io/tsirysndr/otelview --storage duckdb
```

**bun / npm** (downloads the same release binary):

```sh
bun install -g otelview   # or: npm install -g otelview
```

**Pre-built binaries**: grab a tarball from the
[releases page](https://github.com/tsirysndr/otelview/releases); the desktop
app ships there too (`.dmg`, `.AppImage`, `.deb`).

**From source**:

```sh
git clone https://github.com/tsirysndr/otelview && cd otelview
./scripts/fetch-duckdb.sh
(cd ui && bun install && bun run build)
cargo build --release       # → target/release/otelview
```

## Quickstart

```sh
otelview                      # in-memory storage, UI on http://127.0.0.1:4319
otelview --storage duckdb     # persist to ./otelview.duckdb
otelview -c otelview.yaml     # full config

# send something to it
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
```

## Configuration

YAML or TOML — the extension decides. Print all defaults with `otelview --print-config`.

```yaml
# otelview.yaml — every field optional
receivers:
  grpc: { enabled: true, listen: "0.0.0.0:4317" }
  http: { enabled: true, listen: "0.0.0.0:4318" }

auth:
  header: x-otelview-token     # metadata key / HTTP header
  token: sekret                # unset = auth disabled
  protect_api: false           # also require the token on the query API

storage:
  backend: duckdb              # memory | duckdb | jaeger | remote
  memory:
    max_spans: 200000
    max_logs: 200000
    max_metric_points: 500000
  duckdb:
    path: otelview.duckdb      # or ":memory:"
  jaeger:                      # external Jaeger v2 remote-storage backend
    endpoint: "http://127.0.0.1:17271"
    fallback: memory           # logs/metrics live here (not in the Jaeger API)
  remote:                      # another otelview as full 3-signal storage
    endpoint: "http://other-host:4317"
    auth_header: x-otelview-token
    auth_token: sekret

ui:
  listen: "127.0.0.1:4319"
  cors: true                   # allow the desktop app / other origins
  # token: ui-sekret           # optional: require a token to use the web UI

log_level: info
```

The same shape in TOML lives in [`examples/otelview.toml`](examples/otelview.toml).

## The remote-storage APIs

Reads and writes both speak open protocols on the gRPC port:

| Signal  | Write (ingest)                                   | Read                                      |
| ------- | ------------------------------------------------ | ----------------------------------------- |
| traces  | OTLP `TraceService/Export`                       | `jaeger.storage.v2.TraceReader`           |
| logs    | OTLP `LogsService/Export`                        | `otelview.storage.v1.LogReader`           |
| metrics | OTLP `MetricsService/Export`                     | `otelview.storage.v1.MetricReader`        |
| stats   | —                                                | `otelview.storage.v1.Diagnostics`         |

`otelview.storage.v1` ([proto](crates/storage/proto/otelview/storage/v1/storage.proto)) deliberately mirrors the Jaeger v2 design: reader services streaming standard OTLP payloads. Implement it (plus `TraceReader`) and anything can be an otelview backend.

## Development

```sh
./scripts/fetch-duckdb.sh     # once: download the static libduckdb release
(cd ui && bun install && bun run build)
cargo build --release         # single binary at target/release/otelview
cargo test --release

cd ui
bun run dev                   # Vite dev server proxying /api → :4319
bun run test                  # vitest + testing-library + msw
bun run storybook             # component workbench
bun run tauri dev             # desktop shell (point Settings at a remote API)
```

Crate layout: `crates/model` (records), `crates/config`, `crates/storage` (backends + protos), `crates/receiver` (OTLP in + reader servers), `crates/api` (REST + embedded UI), `crates/otelview` (binary). UI: React + Tailwind + HeroUI + Tabler icons, jotai state, VS Code-style layout, Night Rider dark theme.

## Releases

Tagging `v*` builds `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` binaries plus the Tauri desktop bundles and uploads everything to the GitHub release; the docker workflow publishes the multi-arch `ghcr.io/tsirysndr/otelview` image (also runnable on demand via workflow dispatch); the [`otelview`](npm/) npm package installs the matching binary via postinstall.

## License

MIT
