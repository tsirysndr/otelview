import { z } from "zod";

/** Form schemas, kept together so the rules are testable without mounting
 * a component and so error copy stays consistent across forms. */

/** A base URL for an otelview API.
 *
 * Empty is meaningful, not missing: it means "same origin" (or the embedded
 * desktop server). Anything else must parse as an absolute http(s) URL —
 * a typo here is otherwise invisible until every request quietly fails. */
export const baseUrlSchema = z
  .string()
  .trim()
  .refine(
    (v) => {
      if (v === "") return true;
      let u: URL;
      try {
        u = new URL(v);
      } catch {
        return false;
      }
      return u.protocol === "http:" || u.protocol === "https:";
    },
    { message: "must be an http(s) URL, or empty for this same server" },
  );

export const serverProfileSchema = z.object({
  name: z.string().trim().min(1, "give this server a name"),
  baseUrl: baseUrlSchema,
  token: z.string().trim(),
});

export type ServerProfileForm = z.infer<typeof serverProfileSchema>;

/** 24-hour wall clock, as typed into the time-range picker. */
export const timeOfDaySchema = z
  .string()
  .trim()
  .regex(/^([01]?\d|2[0-3]):[0-5]\d$/, "use HH:MM");

export const timeRangeSchema = z
  .object({
    fromTime: timeOfDaySchema,
    toTime: timeOfDaySchema,
  })
  // Ordering across the two fields is only checkable once both parse, so it
  // lives here rather than on either field.
  .refine(
    (v) => {
      const [fh, fm] = v.fromTime.split(":").map(Number);
      const [th, tm] = v.toTime.split(":").map(Number);
      return fh * 60 + fm <= th * 60 + tm;
    },
    { path: ["toTime"], message: "end must be at or after start" },
  );

export type TimeRangeForm = z.infer<typeof timeRangeSchema>;

export const accessTokenSchema = z.object({
  token: z.string().trim().min(1, "paste the token to continue"),
});

export type AccessTokenForm = z.infer<typeof accessTokenSchema>;

/** Name for a saved query. Blank is allowed and falls back to the query
 * text itself, so this only guards against a name that is nothing but
 * whitespace being stored as-is. */
export const savedQueryNameSchema = z.object({
  name: z.string().trim().max(80, "keep it under 80 characters"),
});
