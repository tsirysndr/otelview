# otelview

**The open-source, self-hosted OpenTelemetry viewer — traces, metrics and logs in one fast, beautiful, single binary.**

![otelview](https://raw.githubusercontent.com/tsirysndr/otelview/main/.github/assets/preview.png)

The simplest way to inspect OpenTelemetry data on your own infrastructure. This package downloads the pre-built `otelview` binary for your platform from [GitHub releases](https://github.com/tsirysndr/otelview/releases) — the binary embeds the OTLP receivers, the storage engine (DuckDB, statically linked) and the web UI. No cluster, no JVM, no SaaS bill.

## Used in production

otelview runs in production at [Rocksky](https://rocksky.app), collecting all
three signals from its full fleet of Rust, Node and Go services — millions of
spans, log records and metric points a day into a single DuckDB-backed
instance, with `storage.retention` keeping the database bounded.

## Benchmarks

One seeded run of every storage path against the embedded DuckDB backend, on
an Apple M-series laptop (`cargo bench -p otelview-storage --bench
duck_queries -- 1000000`). Dataset: 1M spans across 250k traces, 1M logs, 2M
metric points — 4M rows, with JSON attributes on every row.

| operation | time | rate |
| --- | --- | --- |
| insert 1M spans | 2.4s | 416k rows/s |
| insert 1M logs | 1.6s | 613k rows/s |
| insert 2M metric points | 3.2s | 632k rows/s |
| find_traces, newest 20 | 7.2ms | |
| find_traces, service + errors only | 3.2ms | |
| find_traces, attribute key=value | 32.6ms | |
| find_traces, attribute substring | 15.3ms | |
| get_trace | 0.6ms | |
| query_logs, newest 300 | 4.8ms | |
| query_logs, body substring | 23.6ms | |
| query_logs, errors only | 4.4ms | |
| metric series (8 series, 4k points) | 42.7ms | |
| list_services / operations / stats | < 9ms | |
| retention sweep, 2.46M expired rows | 1.2s | 2.1M rows/s |

Reads never queue behind ingest: writes serialize on one connection and
queries run on a pool of their own, against a consistent MVCC snapshot.
`cargo bench -p otelview-storage --bench duck_ingest` times the write path on
its own; both benches take a row count as their first argument.

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
- **KQL log search** (Kibana-style) with syntax highlighting and autocomplete, plus metric query functions (rate, increase, aggregations).
- **Storage your way**: in-memory, embedded DuckDB, an external Jaeger v2 remote-storage backend, or another otelview instance.
- **It's a storage server too**: every instance serves the Jaeger v2 `TraceReader` API plus log/metric reader APIs on its gRPC port.
- **Single binary** — the web UI is embedded; configuration is YAML or TOML.

Full documentation, configuration reference and source: [github.com/tsirysndr/otelview](https://github.com/tsirysndr/otelview).

## License

MIT
