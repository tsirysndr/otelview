# otelview + Zitadel

A working single sign-on setup in one `docker compose up`: otelview behind
[Zitadel](https://zitadel.com), with roles, and with multi-factor and
passkeys one toggle away.

The division of labour matters. otelview is an OIDC *relying party*: it
verifies tokens, reads claims and enforces roles. Passwords, second
factors, passkeys, SAML federation, user invitations, account recovery and
audit logs are Zitadel's, and otelview inherits all of them by delegating
login rather than implementing any of it. Turn on passkeys in Zitadel and
otelview has passkeys, with no change here.

## Run it

One line in `/etc/hosts` first, so `zitadel` means the same thing to your
browser and to the containers:

```
127.0.0.1 zitadel
```

Then:

```sh
cd examples/zitadel
docker compose up -d
docker compose logs -f provision   # watch it create the project and app
```

Open <http://localhost:4319> and sign in as **admin@otelview.localhost**
with **Password1!**.

Send it some telemetry and the trace shows up behind the login:

```sh
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 your-app
```

To tear it down, including the data: `docker compose down -v`.

### Why a hostname instead of localhost

An access token carries its issuer's URL and otelview checks it. The
containers reach Zitadel over the compose network; your browser reaches it
through a published port. Both have to arrive at the same name, and
`localhost` inside a container is the container. One `/etc/hosts` line is
the smallest honest fix; in production this is simply your real domain.

## What the provisioning step does

`provision.sh` runs once against a fresh Zitadel and is idempotent
afterwards. It creates:

| | |
| --- | --- |
| Project `otelview` | What the roles and the app belong to |
| Roles `otelview.viewer`, `otelview.admin` | Exactly the strings `viewer_roles` and `admin_roles` match on |
| OIDC app `otelview-web` | Public client, authorization code + PKCE, JWT access tokens |
| Service account `otelview-agent` | For MCP clients and CI, with `otelview.admin` |
| `/shared/otelview.toml` | The config otelview starts with, with the real client and project ids |

Four details are worth knowing, because a hand-made setup without them
fails in ways that look like an otelview bug:

- **`accessTokenType: JWT`** — otelview verifies the token against
  Zitadel's published signing keys, with no network call per request. The
  default (opaque) tokens would need `introspection = true` and a client
  secret instead.
- **The audience scope**, `urn:zitadel:iam:org:project:id:<project>:aud`.
  Without it the token is addressed to Zitadel's own API and otelview
  refuses it as meant for somebody else. Note what Zitadel then puts in
  `aud`: the **project id**, not the application's client id — which is
  why the generated config lists both.
- **The roles scope**, `urn:zitadel:iam:org:projects:roles`. Zitadel does
  not volunteer role claims; without this the login succeeds, the token
  verifies, and otelview refuses the account for having no roles.
- **Role assertion on the project** (`projectRoleAssertion`), plus a
  **grant** per user. A role that exists but is not granted is not in the
  token, and a grant whose project does not assert roles is not either.

All four are set by `provision.sh`. They are listed here because the
symptom of each is the same shrug from otelview, and because a hand-made
Zitadel app will not have them.

## Roles

otelview reads the role claim Zitadel puts in the token and maps it to one
of two levels:

| Role in Zitadel | In otelview | Can |
| --- | --- | --- |
| `otelview.viewer` | viewer | Read traces, logs, metrics, the service map; use MCP |
| `otelview.admin` | admin | All of the above, plus read the instance configuration |

An account with neither is refused after a successful login, and told
which role it is missing.

To give someone access: Zitadel console → your organization → Projects →
otelview → **Authorizations** → New, pick the user and the role. They see
it on their next sign-in.

Change the strings with `viewer_roles` / `admin_roles` in the config if
your organisation already has a role naming scheme. `role_claim` moves the
whole lookup if you are not on Zitadel — Keycloak's is
`realm_access.roles`, and a dotted path reaches into it.

## Multi-factor, passkeys, SAML

All of this is configured in Zitadel and needs no otelview change.

**Passkeys** are on by default in Zitadel's login: users can register one
under their profile, and it works as passwordless sign-in. To require it,
Settings → Login Behavior → *Passwordless* → Required.

**Multi-factor**: Settings → Login Behavior → **Multifactor**. Add
`OTP (authenticator app)` or `U2F`, and set *Force MFA* to require it of
everyone. Existing sessions are unaffected; the next login prompts to
enrol.

**SAML federation** — signing in with a corporate IdP (Okta, Entra,
ADFS): Settings → Identity Providers → SAML SP. Upload the IdP's metadata,
map the attributes, and tick *automatic creation* to let matching users in
without an invitation. otelview sees no difference: the user arrives with
the same OIDC token, and their roles still come from the otelview project.

**Social and OIDC providers** (Google, GitHub, an upstream OIDC) live in
the same place and behave the same way.

The one thing to keep in mind: roles are granted per user in the otelview
project, so federated users need an authorization too. Zitadel *actions*
can grant one automatically based on an IdP attribute if you would rather
not do it by hand.

## Agents and CI

The service account created by the provisioning step is how an MCP client
or a CI job signs in without a human:

```sh
docker compose exec otelview cat /shared/agent-credentials.env
```

```sh
# Fetch a token, then use otelview's API or MCP endpoint with it
TOKEN=$(curl -s -u "$OTELVIEW_AGENT_CLIENT_ID:$OTELVIEW_AGENT_CLIENT_SECRET" \
  -d grant_type=client_credentials \
  -d "scope=openid urn:zitadel:iam:org:projects:roles $OTELVIEW_AUDIENCE_SCOPE" \
  http://zitadel:8080/oauth/v2/token | jq -r .access_token)

curl -s http://localhost:4319/api/services -H "Authorization: Bearer $TOKEN"
```

For MCP, an agent that gets a 401 reads the `WWW-Authenticate` header,
follows the `resource_metadata` URL to
`/.well-known/oauth-protected-resource`, and learns which authorization
server issues tokens for this instance — the discovery half of the MCP
authorization spec. Clients that do not implement that flow can be given a
token directly:

```sh
claude mcp add otelview -- otelview mcp \
  --endpoint http://localhost:4319 --api-token "$TOKEN"
```

## Using Zitadel Cloud instead

Everything on otelview's side is identical; only where Zitadel runs
differs. [docs/zitadel-cloud.md](../../docs/zitadel-cloud.md) is the
console-and-API walkthrough for the hosted service, and `provision.sh`
here works against a cloud instance too if you point `ZITADEL_BASE` at it.

## Before this goes anywhere real

This compose file is deliberately a laptop setup: plain http, a published
Zitadel port, a default password and a master key in the file. The
[production deployment guide](../../docs/deployment.md) covers TLS, real
secrets, where the session state lives, and what to check before pointing
users at it.

The short version:

- Change `ZITADEL_MASTERKEY` and the admin password, and put them in a
  secret store rather than the compose file.
- Put both services behind TLS on real names, and set
  `ZITADEL_EXTERNALSECURE: true` and `secure_cookies = true`.
- Turn `devMode` off on the OIDC app once the redirect URI is https.
- Decide whether `allow_static_token` should stay off. It is off here, so
  Zitadel is the only way in.
