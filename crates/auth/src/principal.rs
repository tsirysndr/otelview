//! Who is asking, and what they are allowed to do.
//!
//! Claims in, a [`Principal`] out. The mapping is configuration rather
//! than code because every provider spells roles differently: Zitadel
//! nests them under `urn:zitadel:iam:org:project:roles`, Keycloak uses
//! `realm_access.roles`, Auth0 a namespaced claim. Only the shape here is
//! fixed.

use std::collections::BTreeSet;

use otelview_config::OidcConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a request is permitted to do.
///
/// Two levels, because otelview reads telemetry and does almost nothing
/// else: everyone who is allowed in can read, and an admin additionally
/// sees the instance's own configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Viewer,
    Admin,
}

impl Role {
    /// Admin is a superset of viewer, so a check for viewer passes for an
    /// admin without every call site having to remember that.
    pub fn satisfies(self, required: Role) -> bool {
        self >= required
    }

    /// What this role may do.
    ///
    /// Endpoints ask for a [`Permission`] rather than a role, so adding a
    /// role later is a change here and nowhere else.
    pub fn grants(self, permission: Permission) -> bool {
        match permission {
            // Reading telemetry is the job; anyone allowed in may do it.
            Permission::ReadTelemetry | Permission::UseMcp => true,
            // The configuration names storage paths, endpoints and the
            // shape of the deployment. Redacted, but still not for
            // everyone who can read a trace.
            Permission::ReadConfig | Permission::Administer => self == Role::Admin,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Admin => "admin",
        }
    }
}

/// A thing a caller may or may not do.
///
/// Deliberately coarse: otelview reads telemetry and almost nothing else,
/// so a permission per endpoint would be ceremony. These are the
/// distinctions that actually matter to a deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Search traces, logs and metrics; read the service map.
    ReadTelemetry,
    /// Read the instance's own configuration.
    ReadConfig,
    /// Call the MCP endpoint.
    UseMcp,
    /// Reserved for anything that changes the instance. Nothing does yet,
    /// and the name is here so that the first thing which does has an
    /// obvious place to sit rather than inventing its own check.
    Administer,
}

/// An authenticated caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    /// The provider's stable id for this user — `sub`. The only field
    /// safe to key anything on: names and emails change.
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    /// Raw role strings as the provider sent them, for display and
    /// debugging. Authorization uses `role` below.
    #[serde(default)]
    pub roles: Vec<String>,
    pub role: Role,
    /// How this caller proved who they are.
    pub via: Credential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Credential {
    /// A browser session cookie from the login flow.
    Session,
    /// An OIDC access token on the request.
    BearerToken,
    /// The pre-shared `ui.token`/`auth.token`, for CI and scripts.
    StaticToken,
}

impl Principal {
    /// The caller behind a static token: allowed everything, named as
    /// nobody, because a shared secret identifies a deployment rather
    /// than a person.
    pub fn static_token() -> Self {
        Self {
            subject: "static-token".into(),
            name: Some("static token".into()),
            email: None,
            organization: None,
            roles: Vec::new(),
            role: Role::Admin,
            via: Credential::StaticToken,
        }
    }

    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    /// Whether this caller may do `permission`.
    pub fn can(&self, permission: Permission) -> bool {
        self.role.grants(permission)
    }

    /// A short label for logs: never the email, which is personal data
    /// that does not belong in an access log by default.
    pub fn label(&self) -> &str {
        &self.subject
    }

    /// Fill in who this is from an id token's claims.
    ///
    /// Display fields only. Authorization stays on the access token, so
    /// a session and an API call with the same account resolve to the
    /// same permissions rather than to whichever token was richer.
    ///
    /// Needed because an access token is not obliged to carry `name` or
    /// `email` — Zitadel does not — and a status bar showing a numeric
    /// subject id helps nobody.
    pub fn enrich_display(&mut self, id_token_claims: &Value) {
        if self.name.is_none() {
            self.name = string_claim(id_token_claims, "name")
                .or_else(|| string_claim(id_token_claims, "preferred_username"));
        }
        if self.email.is_none() {
            self.email = string_claim(id_token_claims, "email");
        }
    }
}

/// Why a set of claims does not yield a principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denied {
    /// The token is fine but this user may not use this instance.
    MissingRole {
        required: Vec<String>,
        found: Vec<String>,
    },
    /// The user belongs to an organisation this instance does not serve.
    ForeignOrganization { found: Option<String> },
    /// No `sub`, so there is nobody to be.
    NoSubject,
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Denied::MissingRole { required, found } => write!(
                f,
                "this account has none of the roles this instance requires \
                 (needs one of [{}], has [{}])",
                required.join(", "),
                found.join(", ")
            ),
            Denied::ForeignOrganization { found } => write!(
                f,
                "this account belongs to organisation {}, which this instance does not serve",
                found.as_deref().unwrap_or("<none>")
            ),
            Denied::NoSubject => write!(f, "the token carries no subject claim"),
        }
    }
}

/// Build a principal from verified token claims.
///
/// The token's signature, issuer, audience and expiry are somebody else's
/// job ([`crate::token`]); by the time claims arrive here they are known
/// to be genuine, and the only question left is whether this instance
/// serves this person.
pub fn from_claims(cfg: &OidcConfig, claims: &Value, via: Credential) -> Result<Principal, Denied> {
    let subject = claims
        .get("sub")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(Denied::NoSubject)?
        .to_string();

    let organization = string_claim(claims, &cfg.organization_claim);
    if !cfg.allowed_organizations.is_empty() {
        let allowed = organization
            .as_deref()
            .is_some_and(|org| cfg.allowed_organizations.iter().any(|a| a == org));
        if !allowed {
            return Err(Denied::ForeignOrganization {
                found: organization,
            });
        }
    }

    let roles = roles_from(claims, &cfg.role_claim);
    let is_admin = intersects(&roles, &cfg.admin_roles);
    let is_viewer = intersects(&roles, &cfg.viewer_roles);

    // No configured roles means "anyone the provider admits", which is
    // right for one team behind one provider and wrong for a shared one.
    let role = if is_admin {
        Role::Admin
    } else if is_viewer || cfg.viewer_roles.is_empty() && cfg.admin_roles.is_empty() {
        Role::Viewer
    } else {
        let mut required = cfg.viewer_roles.clone();
        required.extend(cfg.admin_roles.iter().cloned());
        return Err(Denied::MissingRole {
            required,
            found: roles,
        });
    };

    Ok(Principal {
        subject,
        name: string_claim(claims, "name").or_else(|| string_claim(claims, "preferred_username")),
        email: string_claim(claims, "email"),
        organization,
        roles,
        role,
        via,
    })
}

fn intersects(have: &[String], want: &[String]) -> bool {
    !want.is_empty() && have.iter().any(|h| want.iter().any(|w| w == h))
}

/// A claim that may be addressed by a dotted path, so `realm_access.roles`
/// reaches into a nested object the way Keycloak needs.
fn claim<'a>(claims: &'a Value, path: &str) -> Option<&'a Value> {
    // Dots appear *inside* provider claim names too — Zitadel's
    // `urn:zitadel:iam:org:project:roles` has none, but Auth0 namespaces
    // look like `https://example.com/roles`. A whole-key match wins
    // before the path is split, so a literal key containing dots is
    // still reachable.
    if let Some(v) = claims.get(path) {
        return Some(v);
    }
    let mut cur = claims;
    for segment in path.split('.') {
        cur = cur.get(segment)?;
    }
    Some(cur)
}

fn string_claim(claims: &Value, path: &str) -> Option<String> {
    claim(claims, path).and_then(|v| match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// Roles out of a claim, in every shape providers actually use.
///
/// Zitadel sends an object whose *keys* are the role names and whose
/// values map organisation ids to domains; a plain array is what most
/// others send; a space-separated string is what a few do. All three are
/// accepted because getting this wrong locks everyone out.
fn roles_from(claims: &Value, path: &str) -> Vec<String> {
    let value = match claim(claims, path) {
        Some(v) => v,
        // Zitadel writes roles under a *project-scoped* name —
        // `urn:zitadel:iam:org:project:<project id>:roles` — in tokens
        // issued to a service account, and under the unscoped name in
        // others. Rather than make every operator discover that by
        // getting locked out, an absent claim falls back to any claim of
        // that shape.
        None => match zitadel_project_roles(claims) {
            Some(v) => v,
            None => return Vec::new(),
        },
    };
    let mut roles: BTreeSet<String> = BTreeSet::new();
    match value {
        Value::Object(map) => roles.extend(map.keys().cloned()),
        Value::Array(items) => {
            roles.extend(items.iter().filter_map(Value::as_str).map(str::to_string))
        }
        Value::String(s) => roles.extend(s.split_whitespace().map(str::to_string)),
        _ => {}
    }
    roles.into_iter().collect()
}

/// The first claim named like `urn:zitadel:iam:org:project:<id>:roles`.
fn zitadel_project_roles(claims: &Value) -> Option<&Value> {
    claims.as_object()?.iter().find_map(|(key, value)| {
        let rest = key.strip_prefix("urn:zitadel:iam:org:project:")?;
        // `<id>:roles`, and not the unscoped `roles` which was already
        // tried as the configured claim.
        rest.strip_suffix(":roles")
            .filter(|id| !id.is_empty())
            .map(|_| value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> OidcConfig {
        OidcConfig {
            viewer_roles: vec!["otelview.viewer".into()],
            admin_roles: vec!["otelview.admin".into()],
            ..Default::default()
        }
    }

    /// Zitadel's role claim is an object keyed by role name.
    #[test]
    fn zitadel_roles_are_read_from_the_object_keys() {
        let claims = json!({
            "sub": "user-1",
            "email": "a@example.com",
            "urn:zitadel:iam:org:project:roles": {
                "otelview.viewer": {"orgid": "example.localhost"}
            }
        });
        let p = from_claims(&cfg(), &claims, Credential::BearerToken).unwrap();
        assert_eq!(p.subject, "user-1");
        assert_eq!(p.roles, vec!["otelview.viewer".to_string()]);
        assert_eq!(p.role, Role::Viewer);
        assert!(!p.is_admin());
    }

    /// What Zitadel actually issues to a service account, discovered by
    /// running one: the roles claim carries the project id in its name.
    #[test]
    fn zitadel_project_scoped_role_claims_are_found_too() {
        let claims = json!({
            "sub": "machine-1",
            "urn:zitadel:iam:org:project:391939250892832771:roles": {
                "otelview.admin": {"391939233679474691": "otelview.localhost"}
            }
        });
        let p = from_claims(&cfg(), &claims, Credential::BearerToken).unwrap();
        assert_eq!(p.roles, vec!["otelview.admin".to_string()]);
        assert!(p.is_admin());
    }

    /// The configured claim still wins when both are present.
    #[test]
    fn the_configured_claim_takes_precedence_over_the_fallback() {
        let claims = json!({
            "sub": "u",
            "urn:zitadel:iam:org:project:roles": {"otelview.viewer": {}},
            "urn:zitadel:iam:org:project:999:roles": {"otelview.admin": {}}
        });
        let p = from_claims(&cfg(), &claims, Credential::Session).unwrap();
        assert_eq!(p.role, Role::Viewer, "the configured claim is the answer");
    }

    #[test]
    fn a_claim_that_merely_looks_similar_is_not_mistaken_for_roles() {
        assert!(zitadel_project_roles(&json!({"urn:zitadel:iam:org:project:roles": {}})).is_none());
        assert!(zitadel_project_roles(&json!({"urn:zitadel:iam:org:project:1:aud": {}})).is_none());
        assert!(zitadel_project_roles(&json!({"roles": {}})).is_none());
        assert!(
            zitadel_project_roles(&json!({"urn:zitadel:iam:org:project:1:roles": {}})).is_some()
        );
    }

    #[test]
    fn an_array_or_a_space_separated_string_works_too() {
        let mut c = cfg();
        c.role_claim = "roles".into();
        let array = json!({"sub": "u", "roles": ["otelview.admin"]});
        assert!(from_claims(&c, &array, Credential::BearerToken)
            .unwrap()
            .is_admin());
        let spaced = json!({"sub": "u", "roles": "otelview.viewer otelview.admin"});
        assert!(from_claims(&c, &spaced, Credential::BearerToken)
            .unwrap()
            .is_admin());
    }

    /// Keycloak nests roles; a dotted claim path reaches them.
    #[test]
    fn a_nested_claim_path_is_followed() {
        let mut c = cfg();
        c.role_claim = "realm_access.roles".into();
        let claims = json!({"sub": "u", "realm_access": {"roles": ["otelview.admin"]}});
        assert!(from_claims(&c, &claims, Credential::BearerToken)
            .unwrap()
            .is_admin());
    }

    /// Auth0-style claim names contain dots and must not be split.
    #[test]
    fn a_literal_claim_name_containing_dots_still_resolves() {
        let mut c = cfg();
        c.role_claim = "https://otelview.dev/roles".into();
        let claims = json!({"sub": "u", "https://otelview.dev/roles": ["otelview.viewer"]});
        let p = from_claims(&c, &claims, Credential::BearerToken).unwrap();
        assert_eq!(p.role, Role::Viewer);
    }

    #[test]
    fn admin_outranks_viewer_and_satisfies_it() {
        let claims = json!({
            "sub": "u",
            "urn:zitadel:iam:org:project:roles": {"otelview.admin": {}}
        });
        let p = from_claims(&cfg(), &claims, Credential::Session).unwrap();
        assert_eq!(p.role, Role::Admin);
        assert!(p.role.satisfies(Role::Viewer));
        assert!(p.role.satisfies(Role::Admin));
        assert!(!Role::Viewer.satisfies(Role::Admin));
    }

    #[test]
    fn a_user_without_the_required_role_is_refused() {
        let claims = json!({
            "sub": "u",
            "urn:zitadel:iam:org:project:roles": {"some.other.app": {}}
        });
        let err = from_claims(&cfg(), &claims, Credential::BearerToken).unwrap_err();
        assert!(matches!(err, Denied::MissingRole { .. }));
        // The message names what was needed and what was found, because
        // the alternative is a support ticket.
        let msg = err.to_string();
        assert!(
            msg.contains("otelview.viewer") && msg.contains("some.other.app"),
            "{msg}"
        );
    }

    /// With no roles configured, anyone the provider admits gets in —
    /// the single-team default.
    #[test]
    fn no_configured_roles_admits_any_authenticated_user() {
        let c = OidcConfig::default();
        let p = from_claims(&c, &json!({"sub": "u"}), Credential::Session).unwrap();
        assert_eq!(p.role, Role::Viewer);
    }

    #[test]
    fn an_organization_allowlist_is_enforced() {
        let mut c = cfg();
        c.allowed_organizations = vec!["org-123".into()];
        let outsider = json!({
            "sub": "u",
            "urn:zitadel:iam:org:id": "org-999",
            "urn:zitadel:iam:org:project:roles": {"otelview.viewer": {}}
        });
        assert!(matches!(
            from_claims(&c, &outsider, Credential::BearerToken).unwrap_err(),
            Denied::ForeignOrganization { .. }
        ));

        let insider = json!({
            "sub": "u",
            "urn:zitadel:iam:org:id": "org-123",
            "urn:zitadel:iam:org:project:roles": {"otelview.viewer": {}}
        });
        let p = from_claims(&c, &insider, Credential::BearerToken).unwrap();
        assert_eq!(p.organization.as_deref(), Some("org-123"));
    }

    /// A token with no `sub` identifies nobody, whatever else it carries.
    #[test]
    fn a_token_without_a_subject_is_refused() {
        let claims = json!({"email": "a@example.com"});
        assert_eq!(
            from_claims(&cfg(), &claims, Credential::BearerToken).unwrap_err(),
            Denied::NoSubject
        );
    }

    #[test]
    fn a_name_falls_back_to_the_preferred_username() {
        let claims = json!({"sub": "u", "preferred_username": "ada"});
        let p = from_claims(&OidcConfig::default(), &claims, Credential::Session).unwrap();
        assert_eq!(p.name.as_deref(), Some("ada"));
    }

    #[test]
    fn display_fields_come_from_the_id_token_when_the_access_token_lacks_them() {
        // Exactly what Zitadel issues: an access token with roles and no
        // profile, an id token with the profile.
        let access = json!({
            "sub": "u",
            "urn:zitadel:iam:org:project:roles": {"otelview.admin": {}}
        });
        let mut p = from_claims(&cfg(), &access, Credential::Session).unwrap();
        assert!(p.name.is_none() && p.email.is_none());

        p.enrich_display(&json!({"name": "Ada Lovelace", "email": "ada@example.com"}));
        assert_eq!(p.name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(p.email.as_deref(), Some("ada@example.com"));
        // And it is display only: the role is still the access token's.
        assert_eq!(p.role, Role::Admin);
    }

    /// The access token wins where it said something, so one token
    /// cannot quietly rename the user the other identified.
    #[test]
    fn enriching_never_overwrites_what_was_already_there() {
        let access = json!({"sub": "u", "name": "From Access", "email": "access@example.com"});
        let mut p = from_claims(&OidcConfig::default(), &access, Credential::Session).unwrap();
        p.enrich_display(&json!({"name": "From Id", "email": "id@example.com"}));
        assert_eq!(p.name.as_deref(), Some("From Access"));
        assert_eq!(p.email.as_deref(), Some("access@example.com"));
    }

    #[test]
    fn permissions_follow_from_the_role() {
        assert!(Role::Viewer.grants(Permission::ReadTelemetry));
        assert!(Role::Viewer.grants(Permission::UseMcp));
        // A viewer can read every trace in the instance but not what the
        // instance is plugged into.
        assert!(!Role::Viewer.grants(Permission::ReadConfig));
        assert!(!Role::Viewer.grants(Permission::Administer));

        for p in [
            Permission::ReadTelemetry,
            Permission::UseMcp,
            Permission::ReadConfig,
            Permission::Administer,
        ] {
            assert!(Role::Admin.grants(p), "admin should grant {p:?}");
        }
    }

    #[test]
    fn a_principal_answers_for_its_own_role() {
        let claims = json!({
            "sub": "u",
            "urn:zitadel:iam:org:project:roles": {"otelview.viewer": {}}
        });
        let viewer = from_claims(&cfg(), &claims, Credential::Session).unwrap();
        assert!(viewer.can(Permission::ReadTelemetry));
        assert!(!viewer.can(Permission::ReadConfig));

        assert!(Principal::static_token().can(Permission::ReadConfig));
    }

    #[test]
    fn the_static_token_principal_is_an_admin_with_no_identity() {
        let p = Principal::static_token();
        assert!(p.is_admin());
        assert_eq!(p.via, Credential::StaticToken);
        assert!(p.email.is_none());
    }
}
