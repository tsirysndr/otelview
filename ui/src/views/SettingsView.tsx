import { Button, Input } from "@heroui/react";
import {
  IconCheck,
  IconDeviceFloppy,
  IconPlugConnected,
  IconPlus,
  IconTrash,
} from "@tabler/icons-react";
import { useState } from "react";
import { Controller, useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { fieldProps, plainTextField } from "../lib/inputProps";
import { describeTarget, newProfileId, type ServerProfile } from "../lib/profiles";
import { serverProfileSchema, type ServerProfileForm } from "../lib/schemas";
import { useServerProfiles } from "../hooks/useProfiles";
import { Field } from "../components/Field";

/** Probe a server without touching the shared api client, so testing a
 * profile never disturbs the one currently in use. */
async function probe(p: ServerProfile): Promise<string> {
  try {
    const url = (p.baseUrl.trim().replace(/\/+$/, "") || "") + "/api/stats";
    const headers: Record<string, string> = {};
    if (p.token.trim()) headers["authorization"] = `Bearer ${p.token.trim()}`;
    const resp = await fetch(new URL(url, window.location.origin), { headers });
    return resp.ok ? "connection OK ✓" : `failed: HTTP ${resp.status}`;
  } catch (e) {
    return `failed: ${e instanceof Error ? e.message : String(e)}`;
  }
}

function ProfileEditor({
  profile,
  isActive,
  canRemove,
  onSave,
  onRemove,
  onUse,
}: {
  profile: ServerProfile;
  isActive: boolean;
  canRemove: boolean;
  onSave: (p: ServerProfile) => void;
  onRemove: () => void;
  onUse: () => void;
}) {
  const [status, setStatus] = useState<string | null>(null);
  const {
    control,
    handleSubmit,
    getValues,
    formState: { errors, isDirty },
  } = useForm<ServerProfileForm>({
    resolver: zodResolver(serverProfileSchema),
    defaultValues: {
      name: profile.name,
      baseUrl: profile.baseUrl,
      token: profile.token,
    },
    // Validate as you type. With onBlur the error only cleared when the
    // field lost focus — which is the same event as reaching for Save, so
    // the message vanished, the layout shifted, and the click was swallowed.
    mode: "onChange",
  });

  const save = handleSubmit((values) => {
    onSave({ ...profile, ...values });
    setStatus("saved ✓");
    setTimeout(() => setStatus(null), 2000);
  });

  return (
    <form
      onSubmit={save}
      className={`flex flex-col gap-3 rounded-lg border p-3 ${
        isActive ? "border-neon-cyan/60 bg-content1" : "border-divider"
      }`}
    >
      <div className="flex items-center gap-2">
        <Field label="name" className="flex-1">
          <Controller
            name="name"
            control={control}
            render={({ field }) => (
              <Input
                {...plainTextField}
                {...fieldProps}
                aria-label={`Profile name for ${profile.name}`}
                placeholder="production"
                isInvalid={!!errors.name}
                errorMessage={errors.name?.message}
                value={field.value}
                onValueChange={field.onChange}
                onBlur={field.onBlur}
              />
            )}
          />
        </Field>
        {isActive ? (
          <span className="mt-4 flex shrink-0 items-center gap-1 text-[11px] text-neon-cyan">
            <IconCheck size={13} /> in use
          </span>
        ) : (
          <Button
            type="button"
            className="mt-4 shrink-0"
            size="sm"
            variant="flat"
            color="secondary"
            onPress={onUse}
          >
            use
          </Button>
        )}
      </div>
      <Field label="API base URL">
        <Controller
          name="baseUrl"
          control={control}
          render={({ field }) => (
            <Input
              {...plainTextField}
              {...fieldProps}
              aria-label={`API base URL for ${profile.name}`}
              placeholder="http://127.0.0.1:4319 (empty = same origin)"
              isInvalid={!!errors.baseUrl}
              errorMessage={errors.baseUrl?.message}
              value={field.value}
              onValueChange={field.onChange}
              onBlur={field.onBlur}
            />
          )}
        />
      </Field>
      <Field label="API token">
        <Controller
          name="token"
          control={control}
          render={({ field }) => (
            <Input
              {...plainTextField}
              {...fieldProps}
              aria-label={`API token for ${profile.name}`}
              placeholder="only if auth.protect_api is enabled"
              type="password"
              isInvalid={!!errors.token}
              errorMessage={errors.token?.message}
              value={field.value}
              onValueChange={field.onChange}
              onBlur={field.onBlur}
            />
          )}
        />
      </Field>
      <div className="flex items-center gap-2">
        <Button
          type="submit"
          color="primary"
          size="sm"
          isDisabled={!isDirty}
          startContent={<IconDeviceFloppy size={15} />}
        >
          save
        </Button>
        <Button
          type="button"
          variant="flat"
          size="sm"
          startContent={<IconPlugConnected size={15} />}
          onPress={async () => {
            setStatus("testing…");
            // Probe exactly what is on screen, so a URL can be checked
            // before it is committed to the profile.
            setStatus(await probe({ ...profile, ...getValues() }));
          }}
        >
          test connection
        </Button>
        {canRemove && (
          <Button
            type="button"
            variant="light"
            size="sm"
            color="danger"
            aria-label={`Remove ${profile.name}`}
            startContent={<IconTrash size={15} />}
            onPress={onRemove}
          >
            remove
          </Button>
        )}
        {status && <span className="text-xs text-default-500">{status}</span>}
      </div>
    </form>
  );
}

/** API connection settings: the servers this UI can point at. Mainly for the
 * Tauri desktop app and for anyone juggling prod/staging; the web build
 * defaults to a single same-origin profile. */
export function SettingsView() {
  const { profiles, active, save, saveAndSwitch, switchTo, remove } =
    useServerProfiles();

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4 p-6">
      <div>
        <h2 className="text-sm font-semibold uppercase tracking-wider text-default-500">
          servers
        </h2>
        <p className="mt-1 text-xs text-default-500">
          Where this UI reads data from. Leave a URL empty for the default: the
          server that serves this page (web), or the local instance on
          127.0.0.1:4319 (desktop — an embedded DuckDB-backed server starts
          automatically when none is running). Add more to switch between
          instances, e.g.{" "}
          <span className="text-neon-cyan">http://otel.example.com:4319</span>.
          Switching is also available from the command palette (⌘K).
        </p>
        <p className="mt-1 text-[11px] text-default-400">
          Saved in this browser only — tokens included, so avoid shared machines.
        </p>
      </div>

      {profiles.map((p) => (
        <ProfileEditor
          key={p.id}
          profile={p}
          isActive={p.id === active.id}
          canRemove={profiles.length > 1}
          onSave={save}
          onUse={() => switchTo(p.id)}
          onRemove={() => remove(p.id)}
        />
      ))}

      <div>
        <Button
          size="sm"
          variant="flat"
          startContent={<IconPlus size={15} />}
          onPress={() =>
            saveAndSwitch({
              id: newProfileId(),
              name: `server ${profiles.length + 1}`,
              baseUrl: "",
              token: "",
            })
          }
        >
          add server
        </Button>
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
        <p className="mt-2 text-[11px] text-default-400">
          currently reading from{" "}
          <span className="text-neon-cyan">{describeTarget(active)}</span>
        </p>
      </div>
    </div>
  );
}
