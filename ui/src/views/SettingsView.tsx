import { Button, Input } from "@heroui/react";
import { IconDeviceFloppy, IconPlugConnected } from "@tabler/icons-react";
import { useAtom } from "jotai";
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { apiSettingsAtom } from "../state/atoms";
import { setApiConfig } from "../lib/api";
import { fieldProps, plainTextField } from "../lib/inputProps";
import { Field } from "../components/Field";

/** API connection settings — mainly for the Tauri desktop app, which points
 * at a remote otelview server; the web build defaults to same-origin. */
export function SettingsView() {
  const [settings, setSettings] = useAtom(apiSettingsAtom);
  const [baseUrl, setBaseUrl] = useState(settings.baseUrl);
  const [token, setToken] = useState(settings.token);
  const [status, setStatus] = useState<string | null>(null);
  const qc = useQueryClient();

  const save = () => {
    const next = { baseUrl: baseUrl.trim(), token: token.trim() };
    setSettings(next);
    setApiConfig(next);
    qc.clear();
    setStatus("saved ✓");
    setTimeout(() => setStatus(null), 2000);
  };

  const test = async () => {
    setStatus("testing…");
    try {
      const url = (baseUrl.trim().replace(/\/+$/, "") || "") + "/api/stats";
      const headers: Record<string, string> = {};
      if (token.trim()) headers["authorization"] = `Bearer ${token.trim()}`;
      const resp = await fetch(new URL(url, window.location.origin), { headers });
      setStatus(resp.ok ? "connection OK ✓" : `failed: HTTP ${resp.status}`);
    } catch (e) {
      setStatus(`failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4 p-6">
      <div>
        <h2 className="text-sm font-semibold uppercase tracking-wider text-default-500">
          API connection
        </h2>
        <p className="mt-1 text-xs text-default-500">
          Where this UI reads data from. Leave the URL empty to use the server
          that serves this page (single-binary mode). The desktop app must
          point at a remote otelview instance, e.g.{" "}
          <span className="text-neon-cyan">http://otel.example.com:4319</span>.
        </p>
      </div>
      <Field label="API base URL">
        <Input
          {...plainTextField}
          {...fieldProps}
          size="md"
          aria-label="API base URL"
          placeholder="http://127.0.0.1:4319 (empty = same origin)"
          value={baseUrl}
          onValueChange={setBaseUrl}
        />
      </Field>
      <Field label="API token">
        <Input
          {...plainTextField}
          {...fieldProps}
          size="md"
          aria-label="API token"
          placeholder="only if auth.protect_api is enabled"
          type="password"
          value={token}
          onValueChange={setToken}
        />
      </Field>
      <div className="flex items-center gap-2">
        <Button
          color="primary"
          size="sm"
          startContent={<IconDeviceFloppy size={15} />}
          onPress={save}
        >
          save
        </Button>
        <Button
          variant="flat"
          size="sm"
          startContent={<IconPlugConnected size={15} />}
          onPress={test}
        >
          test connection
        </Button>
        {status && <span className="text-xs text-default-500">{status}</span>}
      </div>

      <div className="mt-6 rounded-lg border border-divider bg-content1 p-4 text-xs text-default-500">
        <h3 className="mb-2 font-semibold text-default-600">sending data here</h3>
        <p>
          OTLP gRPC → <span className="text-neon-cyan">:4317</span> · OTLP HTTP →{" "}
          <span className="text-neon-cyan">:4318</span> (protobuf & JSON, gzip ok)
        </p>
        <pre className="mt-2 overflow-x-auto rounded bg-content2 p-2">
{`export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
# with header auth:
export OTEL_EXPORTER_OTLP_HEADERS="x-otelview-token=<token>"`}
        </pre>
      </div>
    </div>
  );
}
