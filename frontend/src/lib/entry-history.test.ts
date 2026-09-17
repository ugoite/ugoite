import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "./i18n";
import {
  actorDisplayNameLookup,
  resolveActorDisplayName,
  revisionActorId,
  shortActorFallback,
} from "./entry-history";

const ACTOR_UUID = "123e4567-e89b-12d3-a456-426614174000";

describe("entry history actor view model", () => {
  beforeEach(() => {
    setLocale("en");
  });

  it("REQ-UX-ENTRY-002: resolves an actor UUID fixture to the member display name", () => {
    const lookup = actorDisplayNameLookup([
      { principal_id: ACTOR_UUID, display_name: "Ada Example" },
    ]);
    expect(
      resolveActorDisplayName({ actor: ACTOR_UUID }, lookup),
    ).toBe("Ada Example");
  });

  it("REQ-UX-ENTRY-002: keeps short human-meaningful actor values as-is", () => {
    expect(resolveActorDisplayName({ actor: "alice" })).toBe("alice");
    expect(resolveActorDisplayName({ updated_by: "creator" })).toBe("creator");
  });

  it("REQ-UX-ENTRY-002: falls back to a stable short form for unresolvable UUIDs", () => {
    expect(shortActorFallback(ACTOR_UUID)).toBe("123e4567");
    expect(resolveActorDisplayName({ actor: ACTOR_UUID })).toBe("123e4567");
    // The short form never leaks the full UUID.
    expect(shortActorFallback(ACTOR_UUID)).not.toContain("426614174000");
  });

  it("REQ-UX-ENTRY-002: reports unknown actors without inventing identity", () => {
    expect(resolveActorDisplayName({})).toBe("Unknown actor");
    expect(resolveActorDisplayName({ actor: "  " })).toBe("Unknown actor");
    expect(shortActorFallback("")).toBe("Unknown actor");
    expect(revisionActorId({})).toBeNull();
  });

  it("REQ-UX-ENTRY-002: keeps the raw actor identity available for event detail", () => {
    expect(revisionActorId({ actor: ACTOR_UUID })).toBe(ACTOR_UUID);
    expect(revisionActorId({ updated_by: "creator" })).toBe("creator");
  });
});
