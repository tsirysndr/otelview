#!/bin/sh
# Teach a fresh Zitadel about otelview: a project, the roles otelview
# checks for, an OIDC application for the browser, and a service account
# for agents and CI. Then write the config otelview starts with.
#
# Idempotent: every step checks for what it is about to create, so `docker
# compose up` on an existing volume changes nothing and still produces a
# config.
set -eu

ZITADEL_BASE="${ZITADEL_BASE:-http://zitadel:8080}"
OTELVIEW_BASE="${OTELVIEW_BASE:-http://localhost:4319}"
ADMIN_USERNAME="${ADMIN_USERNAME:-admin@otelview.localhost}"
PROJECT_NAME="${PROJECT_NAME:-otelview}"
APP_NAME="${APP_NAME:-otelview-web}"
SERVICE_USER="${SERVICE_USER:-otelview-agent}"
# Where the browser reaches the Login V2 container.
# The `/ui/v2/login` suffix is baked into the login container's build,
# so it serves there whatever port it is on.
LOGIN_BASE="${LOGIN_BASE:-http://zitadel:3000/ui/v2/login}"
OUT="${OUT:-/shared/otelview.toml}"
# Overridable so this runs outside the compose file too — against Zitadel
# Cloud, say, where the token is one you made in the console.
PAT_FILE="${PAT_FILE:-/shared/provisioner.pat}"
TEMPLATE="${TEMPLATE:-/otelview.toml.template}"
CREDS_OUT="${CREDS_OUT:-$(dirname "$OUT")/agent-credentials.env}"

# In the compose file this is an alpine container with nothing in it;
# anywhere else curl and jq are already there.
command -v curl >/dev/null 2>&1 || apk add --no-cache curl jq >/dev/null 2>&1 || true
for tool in curl jq; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "$tool is required" >&2
    exit 1
  }
done

[ -r "$PAT_FILE" ] || {
  echo "no token at $PAT_FILE — set PAT_FILE to a file holding one with rights to manage the organization" >&2
  exit 1
}
PAT="$(cat "$PAT_FILE")"
api() {
  method="$1"
  path="$2"
  body="${3:-}"
  if [ -n "$body" ]; then
    curl -sS -X "$method" "$ZITADEL_BASE$path" \
      -H "Authorization: Bearer $PAT" \
      -H "Content-Type: application/json" \
      -d "$body"
  else
    curl -sS -X "$method" "$ZITADEL_BASE$path" -H "Authorization: Bearer $PAT"
  fi
}

# Give a user a role on the project. Without a grant the role never
# reaches a token, so a failure here is reported rather than swallowed —
# an "already exists" is the one answer worth ignoring.
grant() {
  user="$1"
  label="$2"
  response="$(
    api POST "/management/v1/users/$user/grants" \
      "$(jq -nc --arg p "$project_id" '{projectId:$p,roleKeys:["otelview.admin"]}')"
  )"
  if echo "$response" | jq -e '.userGrantId' >/dev/null 2>&1; then
    echo "granted otelview.admin to $label"
  elif echo "$response" | grep -qi "already exists"; then
    echo "$label already has otelview.admin"
  else
    echo "warning: could not grant otelview.admin to $label: $response" >&2
  fi
}

# Zitadel answers its health check before the management API is ready to
# take writes, so the first call is retried rather than trusted.
echo "waiting for the Zitadel management API"
i=0
until api GET /management/v1/projects/_search '{}' | jq -e '.result? // .details?' >/dev/null 2>&1; do
  i=$((i + 1))
  if [ "$i" -gt 60 ]; then
    echo "Zitadel did not become ready" >&2
    api GET /management/v1/projects/_search '{}' >&2 || true
    exit 1
  fi
  sleep 2
done

# ------------------------------------------------------------ login ui
# v4 requires the Login V2 UI, which is a separate container. The core
# only redirects to it, so it has to be told where that container is —
# otherwise the browser is sent to a path the core does not serve and
# gets a bare {"code": 5, "message": "Not Found"}.
api PUT /v2/features/instance "$(
  jq -nc --arg uri "$LOGIN_BASE" '{loginV2:{required:true,baseUri:$uri}}'
)" >/dev/null || echo "warning: could not point Zitadel at the login UI" >&2
echo "login UI at $LOGIN_BASE"

# ---------------------------------------------------------------- project
project_id="$(
  api POST /management/v1/projects/_search '{"queries":[]}' |
    jq -r --arg name "$PROJECT_NAME" '.result[]? | select(.name==$name) | .id' |
    head -n1
)"
if [ -z "$project_id" ] || [ "$project_id" = "null" ]; then
  project_id="$(
    api POST /management/v1/projects "$(jq -nc --arg n "$PROJECT_NAME" '{name:$n}')" |
      jq -r '.id'
  )"
  echo "created project $PROJECT_NAME ($project_id)"
else
  echo "project $PROJECT_NAME already exists ($project_id)"
fi

# Roles only appear in tokens if the project says to assert them. Without
# this, a login succeeds and arrives at otelview with no roles at all,
# which looks exactly like a misconfigured role_claim.
api PUT "/management/v1/projects/$project_id" "$(
  jq -nc --arg n "$PROJECT_NAME" '{
    name: $n,
    projectRoleAssertion: true,
    projectRoleCheck: false,
    hasProjectCheck: false,
    privateLabelingSetting: "PRIVATE_LABELING_SETTING_UNSPECIFIED"
  }'
)" >/dev/null || true

# ------------------------------------------------------------------ roles
# These strings are what otelview matches on: they are `viewer_roles` and
# `admin_roles` in the config written at the end.
for role in "otelview.viewer:Viewer" "otelview.admin:Admin"; do
  key="${role%%:*}"
  name="${role##*:}"
  api POST "/management/v1/projects/$project_id/roles" \
    "$(jq -nc --arg k "$key" --arg n "$name" '{roleKey:$k,displayName:$n}')" >/dev/null || true
done
echo "roles otelview.viewer and otelview.admin exist"

# ------------------------------------------------------------ application
# A public client using PKCE: no secret to leak into a config file or a
# container image, and the code is bound to the browser that asked for it.
app_id="$(
  api POST "/management/v1/projects/$project_id/apps/_search" '{"queries":[]}' |
    jq -r --arg n "$APP_NAME" '.result[]? | select(.name==$n) | .id' | head -n1
)"
if [ -z "$app_id" ] || [ "$app_id" = "null" ]; then
  created="$(
    api POST "/management/v1/projects/$project_id/apps/oidc" "$(
      jq -nc --arg n "$APP_NAME" --arg redirect "$OTELVIEW_BASE/auth/callback" \
        --arg logout "$OTELVIEW_BASE/" '{
        name: $n,
        redirectUris: [$redirect],
        postLogoutRedirectUris: [$logout],
        responseTypes: ["OIDC_RESPONSE_TYPE_CODE"],
        grantTypes: ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
        appType: "OIDC_APP_TYPE_WEB",
        authMethodType: "OIDC_AUTH_METHOD_TYPE_NONE",
        accessTokenType: "OIDC_TOKEN_TYPE_JWT",
        accessTokenRoleAssertion: true,
        idTokenRoleAssertion: true,
        idTokenUserinfoAssertion: true,
        devMode: true
      }'
    )"
  )"
  app_id="$(echo "$created" | jq -r '.appId')"
  client_id="$(echo "$created" | jq -r '.clientId')"
  echo "created OIDC app $APP_NAME ($client_id)"
else
  client_id="$(
    api GET "/management/v1/projects/$project_id/apps/$app_id" |
      jq -r '.app.oidcConfig.clientId'
  )"
  echo "OIDC app $APP_NAME already exists ($client_id)"
fi

# `devMode` allows the plain-http redirect URI this example uses, and
# `accessTokenType: JWT` is what lets otelview verify a token against the
# published keys instead of calling back to Zitadel on every request.

# ------------------------------------------------- roles for the admin
admin_id="$(
  api POST /management/v1/users/_search "$(
    jq -nc --arg u "$ADMIN_USERNAME" '{queries:[{userNameQuery:{userName:$u,method:"TEXT_QUERY_METHOD_EQUALS"}}]}'
  )" | jq -r '.result[0].id // empty'
)"
if [ -n "$admin_id" ]; then
  grant "$admin_id" "$ADMIN_USERNAME"
else
  echo "warning: could not find $ADMIN_USERNAME to grant it a role" >&2
fi

# ------------------------------------------------------- service account
# For agents and CI: a machine user that can fetch its own token with the
# client-credentials grant, carrying otelview.admin the same way a person
# does. This is how an MCP client signs itself in without a human.
machine_id="$(
  api POST /management/v1/users/_search "$(
    jq -nc --arg u "$SERVICE_USER" '{queries:[{userNameQuery:{userName:$u,method:"TEXT_QUERY_METHOD_EQUALS"}}]}'
  )" | jq -r '.result[0].id // empty'
)"
if [ -z "$machine_id" ]; then
  machine_id="$(
    api POST /management/v1/users/machine "$(
      jq -nc --arg u "$SERVICE_USER" \
        '{userName:$u,name:"otelview agent",description:"MCP and CI access",accessTokenType:"ACCESS_TOKEN_TYPE_JWT"}'
    )" | jq -r '.userId'
  )"
  echo "created service account $SERVICE_USER ($machine_id)"
fi
secret_response="$(api PUT "/management/v1/users/$machine_id/secret" '{}')"
client_secret="$(echo "$secret_response" | jq -r '.clientSecret // empty')"
machine_client_id="$(echo "$secret_response" | jq -r '.clientId // empty')"
grant "$machine_id" "$SERVICE_USER"

# ----------------------------------------------------------- the config
# The audience scope is what makes Zitadel put this project's roles into
# the access token and address it to this project — without it the token
# is for Zitadel itself and otelview rightly refuses it.
sed \
  -e "s|@ISSUER@|$ZITADEL_BASE|g" \
  -e "s|@CLIENT_ID@|$client_id|g" \
  -e "s|@PROJECT_ID@|$project_id|g" \
  -e "s|@REDIRECT@|$OTELVIEW_BASE/auth/callback|g" \
  -e "s|@POST_LOGOUT@|$OTELVIEW_BASE/|g" \
  "$TEMPLATE" >"$OUT"

cat >"$CREDS_OUT" <<EOF
# A service account for MCP clients and CI. Fetch a token with:
#   curl -s -u "\$OTELVIEW_AGENT_CLIENT_ID:\$OTELVIEW_AGENT_CLIENT_SECRET" \\
#     -d grant_type=client_credentials \\
#     -d "scope=openid urn:zitadel:iam:org:projects:roles urn:zitadel:iam:org:project:id:$project_id:aud" \\
#     $ZITADEL_BASE/oauth/v2/token | jq -r .access_token
OTELVIEW_AGENT_CLIENT_ID=$machine_client_id
OTELVIEW_AGENT_CLIENT_SECRET=$client_secret
OTELVIEW_PROJECT_ID=$project_id
OTELVIEW_AUDIENCE_SCOPE=urn:zitadel:iam:org:project:id:$project_id:aud
OTELVIEW_ISSUER=$ZITADEL_BASE
EOF

echo
echo "otelview is configured:"
echo "  issuer     $ZITADEL_BASE"
echo "  client id  $client_id"
echo "  project    $project_id"
echo "  config     $OUT"
echo "  agent creds $CREDS_OUT"
