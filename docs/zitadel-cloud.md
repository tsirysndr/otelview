# otelview with Zitadel Cloud

Zitadel Cloud is the hosted version: no database to run, no master key to
keep, TLS and a domain included. This is the end-to-end setup for putting
otelview behind it — the console path for doing it once by hand, and the
API path for doing it repeatably.

For a self-hosted Zitadel on your own machine, see
[`examples/zitadel/`](../examples/zitadel/), which brings the whole thing
up in one `docker compose up`. Everything about otelview's side is the
same; only where Zitadel lives differs.

- [What you end up with](#what-you-end-up-with)
- [1. Create the instance](#1-create-the-instance)
- [2. Create the project](#2-create-the-project)
- [3. Define the roles](#3-define-the-roles)
- [4. Create the application](#4-create-the-application)
- [5. Grant people the roles](#5-grant-people-the-roles)
- [6. Configure otelview](#6-configure-otelview)
- [7. Sign in](#7-sign-in)
- [Service accounts for agents and CI](#service-accounts-for-agents-and-ci)
- [MFA, passkeys and SAML](#mfa-passkeys-and-saml)
- [Doing all of it through the API](#doing-all-of-it-through-the-api)
- [Multiple teams or customers](#multiple-teams-or-customers)
- [Production notes](#production-notes)
- [When something is wrong](#when-something-is-wrong)

## What you end up with

```
browser ──► otelview /auth/login ──► <you>.zitadel.cloud ──► password / MFA / passkey / SAML
   ▲                                                                        │
   └──────────── session cookie ◄── /auth/callback ◄── code ◄───────────────┘
```

otelview verifies the token Zitadel issues, reads the roles out of it, and
keeps a session. Everything about *who* someone is and *how* they proved
it stays in Zitadel.

Four things have to line up, and the rest is detail:

| | |
| --- | --- |
| **Issuer** | `https://<instance>.zitadel.cloud`, matching `iss` in tokens exactly |
| **Redirect URI** | `https://otelview.example.com/auth/callback`, registered in the app |
| **Scopes** | The project audience scope, and the roles scope |
| **Roles** | Defined on the project, granted per user, and asserted into tokens |

## 1. Create the instance

Sign up at [zitadel.com](https://zitadel.com) and create an instance. You
get a domain like `my-company-a1b2c3.zitadel.cloud`; that URL, with
`https://` and no trailing slash, is otelview's `issuer`.

Check it against what Zitadel publishes, because this is the one value
that must match byte for byte:

```sh
curl -s https://my-company-a1b2c3.zitadel.cloud/.well-known/openid-configuration | jq -r .issuer
```

A custom domain (Settings → Domains) changes the issuer. Set it up
*before* configuring otelview, or change both together — tokens minted
under the old issuer stop verifying the moment it moves.

## 2. Create the project

In the console: your organization → **Projects** → **Create New Project**,
named `otelview`.

Open it and turn on, under its settings:

- **Assert Roles on Authentication** — without it, roles never reach a
  token and every login arrives at otelview with no permissions. This is
  the single most common cause of "this account has none of the roles
  this instance requires".
- **Check for Project on Authentication** is optional. It refuses
  sign-in to anyone with no grant on this project, which makes Zitadel
  reject them rather than otelview. Either is fine; the message is
  friendlier from Zitadel.

## 3. Define the roles

In the project → **Roles** → **New**:

| Key | Display name |
| --- | --- |
| `otelview.viewer` | Viewer |
| `otelview.admin` | Admin |

The keys are the strings otelview matches on. Use your own naming scheme
if you have one — `observability.read`, `sre` — and put the same strings
in `viewer_roles` / `admin_roles`.

| otelview role | May |
| --- | --- |
| viewer | Read traces, logs, metrics and the service map; use MCP |
| admin | All of that, plus read the instance's configuration |

## 4. Create the application

In the project → **Applications** → **New**:

- **Type**: Web
- **Authentication method**: **PKCE** — no client secret to store, and
  the authorization code is bound to the browser that asked for it
- **Redirect URI**: `https://otelview.example.com/auth/callback`
- **Post-logout URI**: `https://otelview.example.com/`

Then open the application's **Token Settings** and set:

- **Auth Token Type**: **JWT**. otelview then verifies tokens against
  Zitadel's published keys with no network call per request. Leave it as
  "Bearer Token" (opaque) and otelview has to call Zitadel's
  introspection endpoint on every single request, which needs a client
  secret and adds a round trip to everything.
- **Add user roles to the access token**: on.
- **User roles inside ID token**: on. Harmless, and useful when debugging
  what the provider actually asserted.

Copy the **Client ID**. There is no secret with PKCE.

> Development Mode on the app permits plain-http redirect URIs. Leave it
> off for a real deployment — with Zitadel Cloud your otelview should be
> on https anyway.

## 5. Grant people the roles

A role that exists but is not granted is not in anyone's token.

Organization → **Authorizations** (older consoles: *Project Grants* →
*Authorizations*) → **New**: pick the user, pick the project, tick the
roles.

Do this for yourself first, or your own first login will be refused — by
otelview, correctly, for having no roles.

## 6. Configure otelview

```toml
[auth.oidc]
enabled = true
issuer = "https://my-company-a1b2c3.zitadel.cloud"
client_id = "301234567890123456@otelview"
redirect_url = "https://otelview.example.com/auth/callback"
post_logout_redirect_url = "https://otelview.example.com/"

scopes = [
    "openid",
    "profile",
    "email",
    # Address the token to this project. Zitadel then puts the *project
    # id* in `aud` — which is why both are accepted below.
    "urn:zitadel:iam:org:project:id:301234567890123456:aud",
    # Ask for roles. Without this the token verifies and carries none.
    "urn:zitadel:iam:org:projects:roles",
]

# The app's client id for browser logins, the project id for tokens a
# service account fetched with the audience scope.
audiences = [
    "301234567890123456@otelview",
    "301234567890123456",
]

viewer_roles = ["otelview.viewer"]
admin_roles  = ["otelview.admin"]

session_ttl    = "8h"
secure_cookies = true
allow_static_token = false
```

The project id is in the console URL when the project is open, and in the
client id after the `@`.

otelview checks the discovery document against `issuer` at startup, so a
mismatch fails immediately and says both spellings rather than turning
into a confusing 401 later.

## 7. Sign in

Restart otelview and open it. The sign-in screen offers **sign in**, which
sends the browser to Zitadel and back. `/auth/me` is the ground truth for
what otelview made of you:

```sh
curl -s https://otelview.example.com/auth/me -b cookies.txt | jq
```

```json
{
  "authenticated": true,
  "subject": "301234567890123456",
  "email": "ada@example.com",
  "role": "admin",
  "roles": ["otelview.admin"],
  "via": "session",
  "permissions": { "read_telemetry": true, "read_config": true, "use_mcp": true }
}
```

If `roles` is empty, the problem is in Zitadel, not otelview: see
[When something is wrong](#when-something-is-wrong).

## Service accounts for agents and CI

A human's session is a cookie; an agent needs a token it can fetch
itself. In Zitadel Cloud: Organization → **Service Users** → **New**, with
**Access Token Type: JWT**. Then:

1. Give it a **Client Secret** (on the service user, under *Actions*), or
   a **Key** for the private-key-JWT grant if you would rather not have a
   shared secret at all.
2. Grant it a role, exactly as for a person — Authorizations → New.

```sh
TOKEN=$(curl -s -u "$CLIENT_ID:$CLIENT_SECRET" \
  -d grant_type=client_credentials \
  -d "scope=openid urn:zitadel:iam:org:projects:roles urn:zitadel:iam:org:project:id:$PROJECT_ID:aud" \
  https://my-company-a1b2c3.zitadel.cloud/oauth/v2/token | jq -r .access_token)

curl -s https://otelview.example.com/api/services -H "Authorization: Bearer $TOKEN"
```

The same token works on the MCP endpoint, so an agent gets exactly the
permissions its roles give it:

```sh
claude mcp add otelview -- otelview mcp \
  --endpoint https://otelview.example.com --api-token "$TOKEN"
```

Both scopes are needed. With only the audience scope the token verifies
and carries no roles, and otelview refuses it for having none — which
reads like an otelview problem and is not.

## MFA, passkeys and SAML

All of this is Zitadel's, and none of it touches otelview.

**Passkeys**: Settings → **Login Behavior and Security**. Users can
register one from their profile; set *Passwordless* to required to insist
on it. This is the cheapest real improvement you can make to an
observability stack that now has a single front door.

**Multi-factor**: same screen → **Multifactor**. Add OTP or U2F, and
*Force MFA* to require it. Enrolment happens at the next login.

**SAML federation** with a corporate IdP (Okta, Entra, ADFS, Google
Workspace): Settings → **Identity Providers** → SAML SP. Upload the IdP
metadata, map attributes to Zitadel's fields, and enable automatic
account creation if you want matching users admitted without an
invitation. otelview sees an ordinary OIDC token afterwards.

**Social logins** (Google, GitHub, Microsoft) live in the same place.

Federated users still need a role grant on the otelview project. A Zitadel
**Action** can grant one automatically from an IdP attribute — a group
claim, a department — if doing it by hand does not scale for you.

## Doing all of it through the API

Clicking is fine once. For anything reproducible, Zitadel Cloud exposes
the same management API as the self-hosted version, and
[`examples/zitadel/provision.sh`](../examples/zitadel/provision.sh) is a
working script against it — point it at your cloud instance:

It needs a token with rights to manage the organization: create a service
user with the `ORG_OWNER` role, give it a **personal access token**, and
put that in a file.

```sh
cd examples/zitadel
echo "$YOUR_PAT" > /tmp/zitadel.pat

ZITADEL_BASE=https://my-company-a1b2c3.zitadel.cloud \
OTELVIEW_BASE=https://otelview.example.com \
ADMIN_USERNAME=you@example.com \
PAT_FILE=/tmp/zitadel.pat \
TEMPLATE=./otelview.toml.template \
OUT=./otelview.toml \
sh provision.sh
```

It writes `otelview.toml` with the real ids, and `agent-credentials.env`
beside it. Remove `allow_insecure_issuer` from the generated file — that
line exists for the plain-http compose setup and a cloud instance is
https.

The calls it makes, if you would rather write your own:

| What | Call |
| --- | --- |
| Create the project | `POST /management/v1/projects` |
| Turn on role assertion | `PUT /management/v1/projects/{id}` with `projectRoleAssertion: true` |
| Add a role | `POST /management/v1/projects/{id}/roles` |
| Create the OIDC app | `POST /management/v1/projects/{id}/apps/oidc` |
| Grant a user a role | `POST /management/v1/users/{userId}/grants` |
| Create a service user | `POST /management/v1/users/machine` |
| Give it a secret | `PUT /management/v1/users/{userId}/secret` |

The grant call is nested under the user; a top-level `/users/grants`
answers `Method Not Allowed`, which is easy to mistake for a permissions
problem.

## Multiple teams or customers

Zitadel organizations are the tenant boundary, and otelview can be told to
serve only some of them:

```toml
allowed_organizations = ["301234567890123456"]
organization_claim = "urn:zitadel:iam:org:id"
```

A user from any other organization is refused after a valid login, and
told why. Note that this is a *filter*, not isolation: otelview stores one
pool of telemetry and everyone admitted sees all of it. For genuinely
separate data, run an otelview per tenant — they are one binary each and
can share a central store through `backend = "remote"`.

## Production notes

- **Token lifetime**: Zitadel's default access token is 12 hours. otelview
  re-checks the session against `session_ttl` (8h by default), so the
  shorter of the two wins. Shorten both for a tighter revocation window.
- **Revocation is not instant.** A JWT stays valid until it expires, even
  after the user is disabled in Zitadel. If you need immediate cutoff, set
  `introspection = true` and accept a call to Zitadel per request, or keep
  sessions short.
- **Sessions are in memory.** Restarting otelview signs everyone out, and
  two replicas do not share a session table — use sticky sessions, or one
  instance for the UI.
- **Keep the static token off** (`allow_static_token = false`) once CI has
  moved to a service account, so Zitadel is genuinely the only way in.
- **The client secret, if you have one**, belongs in
  `OTELVIEW_OIDC_CLIENT_SECRET` rather than the config file. With PKCE
  there is none, which is the better answer.
- **`ui.cors`**: turn it off unless the desktop app needs it.

The rest — TLS, storage, retention, backups — is in the
[deployment guide](deployment.md).

## When something is wrong

`/auth/me` first: it shows what otelview received, which separates "the
provider did not say" from "otelview did not read it".

**"the provider at … calls itself X, but auth.oidc.issuer is Y"** — the
configured issuer does not match the discovery document. Copy the value
from `/.well-known/openid-configuration`. A custom domain change does
this.

**"the token is not addressed to this server"** — the audience scope is
missing from `scopes`, or `audiences` does not list the project id.
Zitadel puts the project id in `aud` for project-scoped tokens, not the
client id.

**"this account has none of the roles this instance requires"** — three
distinct causes, identical symptom: the user has no authorization on the
project, the project does not have *Assert Roles on Authentication* on, or
the token was requested without `urn:zitadel:iam:org:projects:roles`.
Check them in that order.

**"this access token is opaque, and auth.oidc.introspection is off"** —
the app's Auth Token Type is still Bearer. Switch it to JWT.

**The redirect comes back with `invalid_request` or a redirect-mismatch** —
the registered URI must match `redirect_url` exactly, including scheme,
host, port and path. `https://otelview.example.com/auth/callback` and
`https://otelview.example.com/auth/callback/` are different URIs.

**A login loop** — the browser returns and is bounced straight back.
`secure_cookies = true` on a site served over plain http drops the cookie
silently; so does a proxy that rewrites `Set-Cookie` or serves the
callback on a different hostname than the one in `redirect_url`.

**Everything is 401 after a restart** — sessions are in memory. One
sign-in fixes it.
