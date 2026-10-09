import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "./i18n";
import {
  actorDisplayNameLookup,
  resolveActorDisplayName,
  revisionActorId,
} from "./entry-history";

const ACTOR_UUID = "123e4567-e89b-12d3-a456-426614174000";

describe("entry history actor view model", () => {
  beforeEach(() => {
    setLocale("en");
  });

  it("REQ-UX-ENTRY-HISTORY-001: resolves an actor ID to the member display name", () => {
    const lookup = actorDisplayNameLookup([
      { principal_id: ACTOR_UUID, display_name: "Ada Example" },
    ]);
    expect(
      resolveActorDisplayName({ actor: ACTOR_UUID }, lookup),
    ).toBe("Ada Example");
  });

  it("REQ-UX-ENTRY-HISTORY-001: uses a neutral label for unresolved actor IDs", () => {
    expect(resolveActorDisplayName({ actor: ACTOR_UUID })).toBe(
      "Unknown actor",
    );
    expect(resolveActorDisplayName({ actor: "alice" })).toBe("Unknown actor");
    expect(resolveActorDisplayName({ updated_by: "creator" })).toBe(
      "Unknown actor",
    );

    setLocale("ja");
    expect(resolveActorDisplayName({ actor: ACTOR_UUID })).toBe("不明な実行者");
  });

  it("REQ-UX-ENTRY-HISTORY-001: hides member names that echo actor IDs", () => {
    const lookup = actorDisplayNameLookup([
      { principal_id: "actor-7", display_name: "Member actor-7" },
      { principal_id: ACTOR_UUID, display_name: ACTOR_UUID.slice(0, 8) },
    ]);

    expect(resolveActorDisplayName({ actor: "actor-7" }, lookup)).toBe(
      "Unknown actor",
    );
    expect(resolveActorDisplayName({ actor: ACTOR_UUID }, lookup)).toBe(
      "Unknown actor",
    );
  });

  it("REQ-UX-ENTRY-HISTORY-001: reports missing actors without inventing identity", () => {
    expect(resolveActorDisplayName({})).toBe("Unknown actor");
    expect(resolveActorDisplayName({ actor: "  " })).toBe("Unknown actor");
    expect(revisionActorId({})).toBeNull();
  });

  it("REQ-UX-ENTRY-HISTORY-001: keeps the raw actor identity available for details", () => {
    expect(revisionActorId({ actor: ACTOR_UUID })).toBe(ACTOR_UUID);
    expect(revisionActorId({ updated_by: "creator" })).toBe("creator");
  });
});
