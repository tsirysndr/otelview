import { useState } from "react";
import { IconCheck, IconCopy } from "@tabler/icons-react";

const TABS: { key: string; label: string; intro: string; code: string }[] = [
  {
    key: "curl",
    label: "curl",
    intro: "Send a first span right now, no SDK needed:",
    code: `curl -X POST http://localhost:4318/v1/traces \\
  -H 'content-type: application/json' -d '{
  "resourceSpans": [{
    "resource": { "attributes": [{ "key": "service.name",
      "value": { "stringValue": "hello-otelview" } }] },
    "scopeSpans": [{ "spans": [{
      "traceId": "5b8efff798038103d269b633813fc60c",
      "spanId": "eee19b7ec3c1b174",
      "name": "GET /hello", "kind": 2,
      "startTimeUnixNano": "'$(date +%s)'000000000",
      "endTimeUnixNano": "'$(($(date +%s)+1))'000000000"
    }] }]
  }]
}'`,
  },
  {
    key: "node",
    label: "Node.js",
    intro: "Auto-instrument a Node app (http, express, pg, redis, …):",
    code: `npm install @opentelemetry/api \\
  @opentelemetry/auto-instrumentations-node

export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
export OTEL_SERVICE_NAME=my-service

node --require @opentelemetry/auto-instrumentations-node/register app.js`,
  },
  {
    key: "python",
    label: "Python",
    intro: "Auto-instrument a Python app (flask, django, requests, …):",
    code: `pip install opentelemetry-distro opentelemetry-exporter-otlp
opentelemetry-bootstrap -a install

export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
export OTEL_SERVICE_NAME=my-service

opentelemetry-instrument python app.py`,
  },
  {
    key: "collector",
    label: "Collector",
    intro: "Forward from an existing OpenTelemetry Collector:",
    code: `# otel-collector config
exporters:
  otlp/otelview:
    endpoint: localhost:4317
    tls:
      insecure: true

service:
  pipelines:
    traces:
      exporters: [otlp/otelview]
    metrics:
      exporters: [otlp/otelview]
    logs:
      exporters: [otlp/otelview]`,
  },
];

/** "Send your first data" tips shown inside empty states. */
export function GettingStarted() {
  const [tab, setTab] = useState("curl");
  const [copied, setCopied] = useState(false);
  const active = TABS.find((t) => t.key === tab)!;

  const copy = () => {
    void navigator.clipboard?.writeText(active.code);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="mt-2 w-full max-w-lg rounded-lg border border-divider bg-content1 text-left">
      <div className="flex items-center gap-1 border-b border-divider px-2 pt-1.5">
        {TABS.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`rounded-t-md px-2.5 py-1 text-xs transition-colors ${
              tab === t.key
                ? "bg-content2 text-neon-cyan"
                : "text-default-500 hover:text-foreground"
            }`}
          >
            {t.label}
          </button>
        ))}
        <span className="flex-1" />
        <span className="pb-1 pr-1 text-[10px] text-default-400">
          otlp grpc :4317 · http :4318
        </span>
      </div>
      <div className="p-3">
        <p className="mb-2 text-xs text-default-500">{active.intro}</p>
        <div className="relative">
          <pre className="max-h-56 overflow-auto rounded bg-content2 p-2.5 pr-9 text-[11px] leading-relaxed text-default-600">
            {active.code}
          </pre>
          <button
            onClick={copy}
            aria-label="Copy snippet"
            className="absolute right-1.5 top-1.5 rounded p-1 text-default-400 transition-colors hover:text-foreground"
          >
            {copied ? (
              <IconCheck size={14} className="text-neon-green" />
            ) : (
              <IconCopy size={14} />
            )}
          </button>
        </div>
        <p className="mt-2 text-[11px] text-default-400">
          data appears here live — no restart needed. add header auth with{" "}
          <span className="text-default-500">
            OTEL_EXPORTER_OTLP_HEADERS="x-otelview-token=&lt;token&gt;"
          </span>
        </p>
      </div>
    </div>
  );
}
