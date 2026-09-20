/** Saved server profiles: which otelview instance the UI is talking to.
 *
 * Kept as pure functions so the migration from the old single-server shape —
 * the part most likely to lose someone's config — is testable on its own.
 * Persisted in localStorage; there is no sync yet, so ids and timestamps
 * exist mainly so a future sync layer has something stable to key on. */

export interface ServerProfile {
  id: string;
  name: string;
  /** Empty means "same origin" (or the embedded server in the desktop app). */
  baseUrl: string;
  token: string;
}

export interface ApiSettings {
  profiles: ServerProfile[];
  activeId: string;
}

export const DEFAULT_PROFILE_ID = "default";

export function newProfileId(): string {
  // randomUUID needs a secure context; fall back for http:// deployments.
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `p-${Math.random().toString(36).slice(2)}${Date.now().toString(36)}`;
}

export function defaultProfile(): ServerProfile {
  return { id: DEFAULT_PROFILE_ID, name: "this server", baseUrl: "", token: "" };
}

export function defaultSettings(): ApiSettings {
  return { profiles: [defaultProfile()], activeId: DEFAULT_PROFILE_ID };
}

function asProfile(raw: unknown, i: number): ServerProfile | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  if (typeof r.baseUrl !== "string" || typeof r.token !== "string") return null;
  return {
    id: typeof r.id === "string" && r.id ? r.id : `${DEFAULT_PROFILE_ID}-${i}`,
    name: typeof r.name === "string" && r.name ? r.name : `server ${i + 1}`,
    baseUrl: r.baseUrl,
    token: r.token,
  };
}

/** Accept whatever is in storage — including the pre-0.1.5 single-server
 * `{baseUrl, token}` shape — and return something always usable. */
export function normalizeSettings(raw: unknown): ApiSettings {
  if (typeof raw !== "object" || raw === null) return defaultSettings();
  const r = raw as Record<string, unknown>;

  // Old shape: one server, stored flat.
  if (!Array.isArray(r.profiles)) {
    if (typeof r.baseUrl === "string" || typeof r.token === "string") {
      return {
        profiles: [
          {
            ...defaultProfile(),
            baseUrl: typeof r.baseUrl === "string" ? r.baseUrl : "",
            token: typeof r.token === "string" ? r.token : "",
          },
        ],
        activeId: DEFAULT_PROFILE_ID,
      };
    }
    return defaultSettings();
  }

  const profiles = r.profiles
    .map(asProfile)
    .filter((p): p is ServerProfile => p !== null);
  if (profiles.length === 0) return defaultSettings();

  const activeId =
    typeof r.activeId === "string" && profiles.some((p) => p.id === r.activeId)
      ? r.activeId
      : profiles[0].id;
  return { profiles, activeId };
}

export function activeProfile(s: ApiSettings): ServerProfile {
  return s.profiles.find((p) => p.id === s.activeId) ?? s.profiles[0] ?? defaultProfile();
}

/** Insert or replace a profile, keeping list order stable. */
export function upsertProfile(s: ApiSettings, p: ServerProfile): ApiSettings {
  const i = s.profiles.findIndex((x) => x.id === p.id);
  const profiles =
    i === -1 ? [...s.profiles, p] : s.profiles.map((x) => (x.id === p.id ? p : x));
  return { ...s, profiles };
}

/** Remove a profile. The last one is never removed — there must always be
 * somewhere to point at — and removing the active one selects a neighbour. */
export function removeProfile(s: ApiSettings, id: string): ApiSettings {
  if (s.profiles.length <= 1) return s;
  const profiles = s.profiles.filter((p) => p.id !== id);
  const activeId = s.activeId === id ? profiles[0].id : s.activeId;
  return { profiles, activeId };
}

/** How a profile's target reads in the UI. */
export function describeTarget(p: ServerProfile): string {
  return p.baseUrl || "same origin";
}
