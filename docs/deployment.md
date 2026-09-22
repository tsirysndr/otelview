# Deploying otelview in production

otelview is one binary holding the OTLP receivers, the storage, the web
UI, the query API and the MCP endpoint. That makes deployment unusually
simple and puts a few decisions in one place: what stores the telemetry,
who may read it, and what stands between it and the network.

This guide is the long version. For a laptop, `otelview --storage duckdb`
is the whole of it.

- [The shape of it](#the-shape-of-it)
- [Storage and retention](#storage-and-retention)
- [TLS and a reverse proxy](#tls-and-a-reverse-proxy)
- [Who may read the telemetry](#who-may-read-the-telemetry)
- [Single sign-on](#single-sign-on)
- [Roles](#roles)
- [Ingest](#ingest)
- [Agents and MCP](#agents-and-mcp)
- [Running more than one](#running-more-than-one)
- [Backups and upgrades](#backups-and-upgrades)
- [Watching otelview itself](#watching-otelview-itself)
- [Pre-flight checklist](#pre-flight-checklist)
- [When something is wrong](#when-something-is-wrong)

## The shape of it

| Port | Serves | Who reaches it |
| --- | --- | --- |
| 4317 | OTLP gRPC ingest, and the storage reader APIs | Your services |
| 4318 | OTLP HTTP ingest | Your services |
| 4319 | Web UI, query API, `/auth/*`, `/mcp` | People and agents |

Ingest and reading are different audiences with different networks and
different credentials. The simplest sound arrangement is: 4317/4318 on an
internal network only, 4319 behind TLS and single sign-on.

State lives in exactly two places: the storage backend, and the in-memory
session table (see [Running more than one](#running-more-than-one)).
Everything else is derived.

## Storage and retention

```toml
[storage]
backend = "duckdb"
retention = "14d"
retention_sweep_interval = "1h"

[storage.duckdb]
path = "/var/lib/otelview/otelview.duckdb"
```

`duckdb` is the production answer for a single instance: one file,
analytical, and linked statically into the binary. Two things to know:

- **One writer.** A DuckDB file belongs to one process. A second otelview
  on the same path will not start, and neither will `otelview mcp
  --storage duckdb` while the server is up — use `--endpoint` for that.
- **Set `retention`.** Attributes are stored as JSON on every row, so an
  instance with no retention grows until the disk says no. A sweep runs
  every `retention_sweep_interval` and deletes at roughly 2M rows/s, so it
  is cheap to run often.

Sizing, from the benchmarks in the README: ingest is ~400k spans/s and
~600k logs/s, searches are single-digit to tens of milliseconds, and reads
never queue behind writes (writes serialize on one connection, queries run
on a pool against an MVCC snapshot). Disk is the constraint, not CPU.

For several instances sharing one store, point them at a central otelview
with `backend = "remote"`; for traces already in a Jaeger v2 backend, use
`backend = "jaeger"`. In both cases retention belongs to whatever owns the
data, and otelview says so at startup rather than pretending to delete.

## TLS and a reverse proxy

otelview speaks plain HTTP. Put a proxy in front for TLS, and bind
otelview to loopback or an internal interface so nothing reaches it
except through that proxy.

```nginx
server {
  listen 443 ssl http2;
  server_name otelview.example.com;

  ssl_certificate     /etc/ssl/otelview.crt;
  ssl_certificate_key /etc/ssl/otelview.key;

  location / {
    proxy_pass http://127.0.0.1:4319;
    proxy_set_header Host              $host;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;

    # Log tailing and long searches: the default 60s read timeout cuts
    # them off mid-answer.
    proxy_read_timeout 300s;
    proxy_buffering    off;
  }
}
```

Caddy needs three lines and gets a certificate itself:

```caddy
otelview.example.com {
  reverse_proxy 127.0.0.1:4319
}
```

Whatever sits in front, the `redirect_url` in the OIDC config must be the
**public** https URL, because that is where the identity provider sends
the browser and what it checks against its own list.

Turn CORS off (`ui.cors = false`) unless the desktop app or another origin
genuinely needs it. It is on by default for the desktop shell.

## Who may read the telemetry

Three independent doors. Decide each one deliberately; they default open
because a local instance should need no ceremony.

| Door | Turn on with | Notes |
| --- | --- | --- |
| OTLP ingest | `auth.token` | A shared secret your services send |
| UI + query API | `auth.oidc` (or `ui.token`) | People |
| MCP | inherits the above | Agents |

Telemetry is not innocuous: spans and logs carry URLs, user ids, SQL and
whatever an exception message happened to include. An open `0.0.0.0:4319`
publishes all of it.

The simplest production setup is single sign-on for people, and a shared
ingest token for services.

## Single sign-on

otelview is an OIDC relying party. It verifies tokens and reads claims;
passwords, second factors, passkeys, SAML federation, user management and
audit logs belong to the provider. Turning on passkeys or requiring MFA is
a change there and needs nothing here.

Two worked setups: [`examples/zitadel/`](../examples/zitadel/) for a
self-hosted Zitadel you can bring up in one command, and
[zitadel-cloud.md](zitadel-cloud.md) for the hosted service. Read one of
them first if you have no provider yet. The configuration:

```toml
[auth.oidc]
enabled = true
issuer = "https://auth.example.com"
client_id = "otelview-web"
redirect_url = "https://otelview.example.com/auth/callback"
post_logout_redirect_url = "https://otelview.example.com/"

scopes = [
    "openid", "profile", "email",
    # Zitadel: address the token to this project, and ask for roles.
    # Both are needed — the first alone produces a token with no roles,
    # which otelview then refuses for having none.
    "urn:zitadel:iam:org:project:id:PROJECT_ID:aud",
    "urn:zitadel:iam:org:projects:roles",
]

# Zitadel addresses a project-scoped token to the project id, while a
# browser login through the app arrives with the app's client id. Listing
# both lets one instance serve people and service accounts.
audiences = ["CLIENT_ID", "PROJECT_ID"]

viewer_roles = ["otelview.viewer"]
admin_roles  = ["otelview.admin"]

session_ttl    = "8h"
secure_cookies = true
allow_static_token = false
```

What to get right:

- **The `issuer` must match the `iss` in the tokens exactly.** otelview
  checks the discovery document against the configured issuer at startup
  and refuses a mismatch, with both spellings in the message, because
  every other symptom of this is worse.
- **Ask for JWT access tokens** where the provider can issue them (in
  Zitadel: `accessTokenType: JWT` plus the audience scope). Then
  verification is a signature check against cached keys, with no network
  call per request. Otherwise set `introspection = true` and a
  `client_secret`, and accept a call to the provider on every request.
- **`redirect_url` is the public URL**, registered with the provider.
- **Keep the secret out of the file**: `OTELVIEW_OIDC_CLIENT_SECRET`
  overrides `client_secret`, and the browser flow uses PKCE and needs no
  secret at all.
- **`secure_cookies = true`** in production. It is the default; turning it
  off sends the session cookie over plain http.
- **`allow_static_token`** decides whether `ui.token` keeps working beside
  SSO. Leave it on while you migrate CI, then turn it off so that the
  provider is the only way in.

The session cookie is `HttpOnly`, `SameSite=Lax`, signed with a key that
exists only in the running process, and carries an opaque id — never the
tokens. Restarting otelview signs everyone out.

### What the flow looks like

`/auth/login` → the provider (password, MFA, passkey, or off to a
federated SAML IdP) → `/auth/callback` with a code → otelview exchanges it
with its PKCE verifier, verifies the access token exactly as it would an
API caller's, maps claims to a role, and sets the session cookie.
`/auth/logout` drops the session here *and* ends it at the provider.

## Roles

```
role claim in the token  →  viewer_roles / admin_roles  →  permissions
```

| | Viewer | Admin |
| --- | --- | --- |
| Read traces, logs, metrics, service map | yes | yes |
| Use MCP | yes | yes |
| Read `/api/config` | no | yes |

An authenticated account with neither role is refused, and told which
roles it is missing. **Leaving both lists empty admits anyone the provider
admits** — right for one team with a dedicated provider, wrong for a
shared one. otelview logs a warning at startup when they are empty.

`role_claim` selects where roles are read from, with a dotted path for
nested claims. Zitadel's default is `urn:zitadel:iam:org:project:roles`;
Keycloak is `realm_access.roles`; Auth0 uses a namespaced claim, and a
literal claim name containing dots still works.

`allowed_organizations` restricts sign-in to particular organisation ids
for a multi-tenant provider.

## Ingest

```toml
[auth]
header = "x-otelview-token"
token = "a-long-random-string"
protect_api = false
```

Every OTLP request must then carry that header; in the SDKs it is
`OTEL_EXPORTER_OTLP_HEADERS=x-otelview-token=…`. `protect_api = true`
additionally makes that token open the query API, which is the
token-only alternative to single sign-on.

Prefer a network boundary as well: ingest on an internal interface, and
the UI as the only public surface.

## Agents and MCP

MCP is served at `/mcp` on the UI port and is guarded by the same
credentials as everything else. With SSO on, an agent may present an OIDC
access token; a 401 carries `WWW-Authenticate` with a `resource_metadata`
URL pointing at `/.well-known/oauth-protected-resource`, which names the
authorization server — that is how a client signs itself in instead of
being handed a token by a person.

For clients that cannot do that, a service account's token or a dedicated
`mcp.token` works:

```toml
[mcp]
enabled = true
path = "/mcp"
# OTELVIEW_MCP_TOKEN overrides this.
```

Two refusals are built in: binding MCP to a non-loopback address with no
token fails at startup, and a request carrying a browser `Origin` from
anywhere but loopback is rejected (the DNS-rebinding mitigation). Set
`mcp.enabled = false` to remove the endpoint.

Everything MCP exposes is read-only.

## Running more than one

One instance is the design. Two things stop you simply scaling the same
configuration out:

- **DuckDB is single-writer.** Several instances cannot share a file. The
  supported shape is one instance owning the storage and others pointed at
  it with `backend = "remote"` — they read, it stores.
- **Sessions are in memory.** A second replica does not know the first
  one's sessions, so a user bounced between them is asked to sign in
  again. Use sticky sessions at the proxy, or keep the UI on one instance.
  Bearer-token callers — agents, CI, the API — are unaffected, since
  nothing about them is stateful.

Restarting signs everyone out. That is a real cost of keeping the binary
dependency-free, and it is measured in one redirect.

## Backups and upgrades

The DuckDB file is the backup. Copy it while otelview is stopped, or use
DuckDB's own `EXPORT DATABASE` against a copy. Nothing else on disk
matters: the config is yours, and sessions and caches are rebuilt.

Upgrades are: stop, replace the binary, start. The schema migrates
forward on open. Read the release notes for a major version. Rolling back
across a schema change means restoring the file you copied first, so copy
it first.

The desktop app and the npm package follow the same version as the
server; an older UI against a newer API is supported within a minor
version, but there is no reason to run one.

## Watching otelview itself

It emits its own telemetry format, so point it at itself or at another
instance:

```sh
OTEL_EXPORTER_OTLP_ENDPOINT=http://otelview-central:4318 \
OTEL_SERVICE_NAME=otelview \
otelview -c /etc/otelview/otelview.toml
```

`log_level` takes a `tracing` filter (`info`, `debug`,
`info,otelview_auth=debug`), and `RUST_LOG` overrides it. The lines worth
alerting on:

| Line | Means |
| --- | --- |
| `retention sweep failed` | Disk or storage trouble; it retries |
| `the identity provider could not be reached` | Logins are failing, and this is not the caller's fault |
| `refused an MCP request with a missing or invalid token` | Someone is trying, or a client is misconfigured |
| `refused an MCP request from a foreign browser origin` | A web page tried to reach a local instance |
| `session table is full` | Something is creating sessions in a loop |

`/api/stats` is a cheap liveness and volume check. The UI's status line
reads the same thing.

## Pre-flight checklist

- [ ] `otelview --print-config` matches what you think you deployed
- [ ] Storage is `duckdb` (or `remote`) with a path on persistent disk
- [ ] `retention` is set, and the sweep is running (it logs)
- [ ] 4319 is behind TLS; 4317/4318 are not public
- [ ] `ui.listen` binds the interface you meant
- [ ] `auth.oidc.issuer` matches the provider's own spelling
- [ ] `redirect_url` is the public https URL and is registered
- [ ] `viewer_roles` / `admin_roles` are set, and someone holds one
- [ ] `secure_cookies = true`
- [ ] `allow_static_token = false`, once CI has moved
- [ ] The client secret comes from the environment, not the file
- [ ] `auth.token` is set for ingest, and your exporters send it
- [ ] MFA or passkeys required at the provider, if that is your policy
- [ ] The DuckDB file is in the backup
- [ ] A test login works from outside your network

## When something is wrong

**"the provider at … calls itself X, but auth.oidc.issuer is Y"** — the
discovery document disagrees with your config. Use exactly the string the
provider publishes, trailing slash and all.

**"the token is not addressed to this server"** — the access token's
audience is not this client. On Zitadel, add the
`urn:zitadel:iam:org:project:id:<project>:aud` scope; elsewhere, configure
an audience for the API and request it. `audiences` in the config accepts
more than one if you are migrating between clients.

**"this access token is opaque, and auth.oidc.introspection is off"** —
the provider issued a reference token rather than a JWT. Ask for JWTs, or
set `introspection = true` with a `client_secret`.

**"this account has none of the roles this instance requires"** — the
login worked and the authorization did not. `/auth/me` shows exactly what
otelview saw. On Zitadel there are three separate reasons this happens,
and all three look identical from here: the user has no *grant* on the
project, the project does not have *role assertion* switched on, or the
token was requested without the `urn:zitadel:iam:org:projects:roles`
scope. Elsewhere, check `role_claim` against where your provider actually
writes them.

**A login loop** — the browser returns and is immediately sent back. Check
`secure_cookies` against whether the site is really https, and that the
proxy is not stripping `Set-Cookie` or serving the callback on a different
host than `redirect_url`.

**"This login has expired or was already used"** — the state has been
spent, which is what makes it a defence against replay. A bookmarked
callback URL does this; start from `/auth/login`.

**Everything is 401 after a restart** — sessions are in memory and this is
expected. One sign-in fixes it.

**"a DuckDB file cannot be opened twice"** — something else holds it.
`otelview mcp --endpoint` instead of `--storage duckdb`.
