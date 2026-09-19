# otelview

**The open-source, self-hosted OpenTelemetry viewer — traces, metrics and logs in one fast, beautiful, single binary.**

![otelview](https://raw.githubusercontent.com/tsirysndr/otelview/main/.github/assets/preview.png)

An alternative to Datadog, Kibana, CloudWatch and SigNoz you can run anywhere. This package downloads the pre-built `otelview` binary for your platform from [GitHub releases](https://github.com/tsirysndr/otelview/releases) — the binary embeds the OTLP receivers, the storage engine (DuckDB, statically linked) and the web UI. No cluster, no JVM, no SaaS bill.

## Install

```sh
npm install -g otelview
```

Supported platforms: macOS (Apple Silicon), Linux (x86_64, arm64).

## Usage

```sh
otelview                      # in-memory storage, UI on http://127.0.0.1:4319
otelview --storage duckdb     # persist to ./otelview.duckdb
otelview -c otelview.yaml     # full YAML/TOML config
otelview --print-config       # print all defaults
```

Then point your apps at it:

```sh
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318   # HTTP (protobuf & JSON, gzip)
# or gRPC on localhost:4317
```

Open **http://127.0.0.1:4319** for the UI: trace search + waterfall, live log tail, and a metrics explorer.

## Screenshots

**Traces** — search with a latency scatter plot, then drill into the waterfall:

![traces](https://raw.githubusercontent.com/tsirysndr/otelview/main/.github/assets/traces.png)

**Logs** — live tail with severity filtering and trace correlation:

![logs](https://raw.githubusercontent.com/tsirysndr/otelview/main/.github/assets/logs.png)

**Metrics** — explorer with multi-series charts for every OTLP metric type:

![metrics](https://raw.githubusercontent.com/tsirysndr/otelview/main/.github/assets/metrics.png)

## Highlights

- **All three signals** over OTLP gRPC (`:4317`) and HTTP (`:4318`), with optional header-token auth.
- **Storage your way**: in-memory, embedded DuckDB, an external Jaeger v2 remote-storage backend, or another otelview instance.
- **It's a storage server too**: every instance serves the Jaeger v2 `TraceReader` API plus log/metric reader APIs on its gRPC port.
- **Single binary** — the web UI is embedded; configuration is YAML or TOML.

Full documentation, configuration reference and source: [github.com/tsirysndr/otelview](https://github.com/tsirysndr/otelview).

## License

MIT
