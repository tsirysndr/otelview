import { describe, expect, it } from "vitest";
import { accessTokenSchema, baseUrlSchema, serverProfileSchema } from "./schemas";

describe("baseUrl", () => {
  it("accepts empty, meaning this same server", () => {
    expect(baseUrlSchema.safeParse("").success).toBe(true);
    expect(baseUrlSchema.safeParse("   ").success).toBe(true);
  });

  it("accepts absolute http(s) URLs, with or without a port or path", () => {
    for (const v of [
      "http://127.0.0.1:4319",
      "https://otel.example.com",
      "https://otel.example.com:4319/base",
    ]) {
      expect(baseUrlSchema.safeParse(v).success, v).toBe(true);
    }
  });

  it("rejects the typos that would otherwise fail silently at request time", () => {
    for (const v of [
      "htp://127.0.0.1:4319",
      "127.0.0.1:4319",
      "otel.example.com",
      "ftp://otel.example.com",
      "javascript:alert(1)",
      "://nope",
    ]) {
      expect(baseUrlSchema.safeParse(v).success, v).toBe(false);
    }
  });

  it("trims before validating", () => {
    const r = baseUrlSchema.safeParse("  http://127.0.0.1:4319  ");
    expect(r.success).toBe(true);
    expect(r.success && r.data).toBe("http://127.0.0.1:4319");
  });
});

describe("server profile", () => {
  it("requires a name", () => {
    const r = serverProfileSchema.safeParse({ name: "  ", baseUrl: "", token: "" });
    expect(r.success).toBe(false);
    expect(r.success === false && r.error.issues[0].message).toMatch(/name/);
  });

  it("accepts a complete profile", () => {
    expect(
      serverProfileSchema.safeParse({
        name: "prod",
        baseUrl: "https://otel.example.com",
        token: "t",
      }).success,
    ).toBe(true);
  });

  it("allows an empty token — auth is optional server-side", () => {
    expect(
      serverProfileSchema.safeParse({ name: "local", baseUrl: "", token: "" }).success,
    ).toBe(true);
  });
});

describe("access token", () => {
  it("requires something non-blank", () => {
    expect(accessTokenSchema.safeParse({ token: "" }).success).toBe(false);
    expect(accessTokenSchema.safeParse({ token: "   " }).success).toBe(false);
    expect(accessTokenSchema.safeParse({ token: "abc" }).success).toBe(true);
  });
});
