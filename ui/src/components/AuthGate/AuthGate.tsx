import { Controller, useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { Button, Input } from "@heroui/react";
import { IconLock, IconShieldLock } from "@tabler/icons-react";
import { useQuery } from "@tanstack/react-query";
import { api, ApiError } from "../../lib/api";
import { useServerProfiles } from "../../hooks/useProfiles";
import { fieldProps, plainTextField } from "../../lib/inputProps";
import { accessTokenSchema, type AccessTokenForm } from "../../lib/schemas";
import { Field } from "../Field";

/** Blocks the workspace behind whatever this server requires. The shell
 * itself is public and every /api call is gated server-side; this screen
 * only collects the credential.
 *
 * Which credential depends on the server, which is why it asks: an
 * instance behind single sign-on sends the browser to its identity
 * provider, one with `ui.token` prompts for that token, and a local
 * instance is not gated at all. */
export function AuthGate({ children }: { children: React.ReactNode }) {
  const { data: authInfo } = useQuery({
    queryKey: ["auth-info"],
    queryFn: api.authInfo,
    // The mode is a property of the deployment, not of the session.
    staleTime: Infinity,
    retry: false,
  });

  const { error, isLoading } = useQuery({
    queryKey: ["auth-check"],
    queryFn: api.stats,
    retry: false,
  });

  const unauthorized = error instanceof ApiError && error.status === 401;
  const forbidden = error instanceof ApiError && error.status === 403;

  if (forbidden) return <NoAccess detail={error.message} />;
  if (!unauthorized) return <>{children}</>;
  if (authInfo?.mode === "oidc") return <SsoSignIn issuer={authInfo.issuer} />;
  return <TokenPrompt isLoading={isLoading} />;
}

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-6">
      {children}
    </div>
  );
}

function Wordmark({ subtitle, icon }: { subtitle: string; icon: React.ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-1.5">
      {icon}
      <h2 className="text-sm font-semibold tracking-widest">
        otel<span className="text-neon-magenta">view</span>
      </h2>
      <p className="max-w-xs text-center text-xs text-default-500">{subtitle}</p>
    </div>
  );
}

/** Sends the browser to the identity provider, and back to wherever it
 * was. A full navigation rather than fetch: the provider will want to
 * show its own pages — a password, a second factor, a passkey prompt — and
 * may redirect on to a federated SAML idp before returning here. */
function SsoSignIn({ issuer }: { issuer?: string }) {
  const returnTo = window.location.pathname + window.location.search;
  const href = `/auth/login?return_to=${encodeURIComponent(returnTo)}`;
  return (
    <Shell>
      <Wordmark
        icon={<IconShieldLock size={36} stroke={1.4} className="text-neon-magenta" />}
        subtitle="this instance uses single sign-on"
      />
      <Button as="a" href={href} color="primary" size="sm" data-testid="sso-sign-in">
        sign in
      </Button>
      {issuer && (
        <p className="max-w-xs break-all text-center text-[11px] text-default-500">
          via {issuer}
        </p>
      )}
    </Shell>
  );
}

/** Signed in, and still not allowed in — a role the instance requires is
 * missing. Retrying the login would not help, so this does not offer it. */
function NoAccess({ detail }: { detail: string }) {
  return (
    <Shell>
      <Wordmark
        icon={<IconLock size={36} stroke={1.4} className="text-danger" />}
        subtitle="your account does not have access to this instance"
      />
      <p className="max-w-sm text-center text-[11px] text-default-500">{detail}</p>
      <Button as="a" href="/auth/logout" variant="bordered" size="sm">
        sign out
      </Button>
    </Shell>
  );
}

function TokenPrompt({ isLoading }: { isLoading: boolean }) {
  const { active, save } = useServerProfiles();
  const {
    control,
    handleSubmit,
    formState: { errors, isValid },
  } = useForm<AccessTokenForm>({
    resolver: zodResolver(accessTokenSchema),
    defaultValues: { token: "" },
    mode: "onChange",
  });

  // Unlocking re-tokens the profile we are already pointed at.
  const submit = handleSubmit(({ token }) => save({ ...active, token }));

  return (
    <Shell>
      <Wordmark
        icon={<IconLock size={36} stroke={1.4} className="text-neon-magenta" />}
        subtitle="this instance requires an access token"
      />
      <form className="flex w-72 flex-col gap-3" onSubmit={submit}>
        <Field label="access token">
          <Controller
            name="token"
            control={control}
            render={({ field }) => (
              <Input
                {...plainTextField}
                {...fieldProps}
                size="md"
                autoFocus
                type="password"
                aria-label="Access token"
                placeholder="paste the ui token"
                isInvalid={!!errors.token}
                errorMessage={errors.token?.message}
                value={field.value}
                onValueChange={field.onChange}
                onBlur={field.onBlur}
              />
            )}
          />
        </Field>
        <Button type="submit" color="primary" size="sm" isDisabled={!isValid}>
          unlock
        </Button>
        {active.token && (
          <p className="text-center text-[11px] text-danger">
            token rejected — check it and try again
          </p>
        )}
      </form>
      {isLoading && <p className="text-[11px] text-default-500">checking…</p>}
    </Shell>
  );
}
