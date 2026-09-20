import { describe, expect, it } from "vitest";
import {
  activeProfile,
  DEFAULT_PROFILE_ID,
  defaultSettings,
  normalizeSettings,
  removeProfile,
  upsertProfile,
  type ApiSettings,
} from "./profiles";

const two: ApiSettings = {
  profiles: [
    { id: "a", name: "prod", baseUrl: "https://prod", token: "t1" },
    { id: "b", name: "staging", baseUrl: "https://staging", token: "" },
  ],
  activeId: "b",
};

describe("migration from the single-server shape", () => {
  it("carries the old baseUrl and token into one profile", () => {
    const s = normalizeSettings({ baseUrl: "https://old", token: "secret" });
    expect(s.profiles).toHaveLength(1);
    expect(s.profiles[0].baseUrl).toBe("https://old");
    expect(s.profiles[0].token).toBe("secret");
    expect(s.activeId).toBe(DEFAULT_PROFILE_ID);
    expect(activeProfile(s).token).toBe("secret");
  });

  it("treats the empty old shape as the default same-origin profile", () => {
    const s = normalizeSettings({ baseUrl: "", token: "" });
    expect(s.profiles).toHaveLength(1);
    expect(s.profiles[0].baseUrl).toBe("");
  });

  it("falls back to defaults for junk in storage", () => {
    for (const junk of [null, undefined, 42, "nope", [], {}]) {
      expect(normalizeSettings(junk)).toEqual(defaultSettings());
    }
  });
});

describe("normalizing the current shape", () => {
  it("round-trips a valid value", () => {
    expect(normalizeSettings(two)).toEqual(two);
  });

  it("drops malformed profiles but keeps the good ones", () => {
    const s = normalizeSettings({
      profiles: [two.profiles[0], { name: "no urls" }, null, 7],
      activeId: "a",
    });
    expect(s.profiles).toHaveLength(1);
    expect(s.profiles[0].id).toBe("a");
  });

  it("repairs an activeId that points at nothing", () => {
    const s = normalizeSettings({ ...two, activeId: "ghost" });
    expect(s.activeId).toBe("a");
    expect(activeProfile(s).name).toBe("prod");
  });

  it("names and ids profiles that lack them", () => {
    const s = normalizeSettings({
      profiles: [{ baseUrl: "https://x", token: "" }],
      activeId: "",
    });
    expect(s.profiles[0].id).toBeTruthy();
    expect(s.profiles[0].name).toBeTruthy();
    expect(s.activeId).toBe(s.profiles[0].id);
  });

  it("returns defaults when every profile is malformed", () => {
    expect(normalizeSettings({ profiles: [null, 1], activeId: "x" })).toEqual(
      defaultSettings(),
    );
  });
});

describe("editing profiles", () => {
  it("upsert adds a new profile and replaces an existing one in place", () => {
    const added = upsertProfile(two, { id: "c", name: "dev", baseUrl: "", token: "" });
    expect(added.profiles.map((p) => p.id)).toEqual(["a", "b", "c"]);

    const edited = upsertProfile(two, { ...two.profiles[0], name: "production" });
    expect(edited.profiles.map((p) => p.id)).toEqual(["a", "b"]);
    expect(edited.profiles[0].name).toBe("production");
  });

  it("removing the active profile selects another", () => {
    const s = removeProfile(two, "b");
    expect(s.profiles.map((p) => p.id)).toEqual(["a"]);
    expect(s.activeId).toBe("a");
  });

  it("removing an inactive profile leaves the selection alone", () => {
    const s = removeProfile(two, "a");
    expect(s.activeId).toBe("b");
  });

  it("refuses to remove the last profile", () => {
    const one: ApiSettings = { profiles: [two.profiles[0]], activeId: "a" };
    expect(removeProfile(one, "a")).toEqual(one);
  });
});
