import { useState } from "react";
import { Button, Input } from "@heroui/react";
import { IconLock } from "@tabler/icons-react";
import { useAtom } from "jotai";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiSettingsAtom } from "../../state/atoms";
import { api, ApiError, setApiConfig } from "../../lib/api";
import { fieldProps, plainTextField } from "../../lib/inputProps";
import { Field } from "../Field";

/** Blocks the workspace behind a token prompt when the server requires one
 * (ui.token in the config). The shell itself is public; every /api call is
 * gated server-side — this screen just collects the token. */
export function AuthGate({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useAtom(apiSettingsAtom);
  const [token, setToken] = useState("");
  const qc = useQueryClient();

  const { error, isLoading } = useQuery({
    queryKey: ["auth-check", settings.token],
    queryFn: api.stats,
    retry: false,
  });

  const unauthorized = error instanceof ApiError && error.status === 401;
  if (!unauthorized) return <>{children}</>;

  const submit = () => {
    const next = { ...settings, token: token.trim() };
    setSettings(next);
    setApiConfig(next);
    qc.invalidateQueries();
  };

  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-6">
      <div className="flex flex-col items-center gap-1.5">
        <IconLock size={36} stroke={1.4} className="text-neon-magenta" />
        <h2 className="text-sm font-semibold tracking-widest">
          otel<span className="text-neon-magenta">view</span>
        </h2>
        <p className="text-xs text-default-500">
          this instance requires an access token
        </p>
      </div>
      <form
        className="flex w-72 flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <Field label="access token">
          <Input
            {...plainTextField}
            {...fieldProps}
            size="md"
            autoFocus
            type="password"
            aria-label="Access token"
            placeholder="paste the ui token"
            value={token}
            onValueChange={setToken}
          />
        </Field>
        <Button type="submit" color="primary" size="sm" isDisabled={!token.trim()}>
          unlock
        </Button>
        {settings.token && (
          <p className="text-center text-[11px] text-danger">
            token rejected — check it and try again
          </p>
        )}
      </form>
      {isLoading && <p className="text-[11px] text-default-500">checking…</p>}
    </div>
  );
}
