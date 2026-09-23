import { describe, expect, it } from "vitest";
import {
  decodeSpaceSegment,
  encodeSpaceSegment,
  spaceBase,
  spaceDashboardPath,
  spaceEntriesPath,
  spaceEntryPath,
  spaceFormEntriesPath,
  spaceHistoryPath,
  spacePath,
  spaceSearchPath,
  spaceSettingsPath,
} from "./space-path";

const ENCODED_FIXTURE = "space%2Fwith%20space";

describe("space-path segment helper (#2849)", () => {
  it("encodes identifiers that require path-segment encoding", () => {
    expect(encodeSpaceSegment("space/with space")).toBe(ENCODED_FIXTURE);
    expect(encodeSpaceSegment("my space/ü+%")).toBe(
      encodeURIComponent("my space/ü+%"),
    );
  });

  it("round-trips the encoded segment fixture", () => {
    expect(decodeSpaceSegment(ENCODED_FIXTURE)).toBe("space/with space");
    expect(decodeSpaceSegment("space/with space")).toBe("space/with space");
    expect(decodeSpaceSegment("default")).toBe("default");
    expect(decodeSpaceSegment("")).toBe("");
    expect(decodeSpaceSegment(undefined)).toBe("");
    // Malformed % sequences never throw; the raw segment is preserved.
    expect(decodeSpaceSegment("100%bad")).toBe("100%bad");
  });

  it("keeps existing URLs intact for plain IDs (deep-link compat)", () => {
    expect(spaceBase("default")).toBe("/spaces/default");
    expect(spaceDashboardPath("default")).toBe("/spaces/default/dashboard");
    expect(spaceEntriesPath("default")).toBe("/spaces/default/entries");
    expect(spaceEntryPath("default", "entry-1")).toBe(
      "/spaces/default/entries/entry-1",
    );
    expect(spaceSearchPath("default")).toBe("/spaces/default/search");
    expect(spaceHistoryPath("default")).toBe("/spaces/default/history");
    expect(spaceSettingsPath("default")).toBe("/spaces/default/settings");
  });

  it("addresses a Form's Entries through the canonical Form workspace path", () => {
    expect(spaceFormEntriesPath("default", "Notes")).toBe(
      "/spaces/default/forms/Notes/entries",
    );
    expect(spaceFormEntriesPath("default", "My Form")).toBe(
      "/spaces/default/forms/My%20Form/entries",
    );
    const spaceId = "space/with space";
    expect(spaceFormEntriesPath(spaceId, "Notes")).toBe(
      `/spaces/${ENCODED_FIXTURE}/forms/Notes/entries`,
    );
  });

  it("builds encoded sibling routes from one helper", () => {
    const spaceId = "space/with space";
    expect(spaceDashboardPath(spaceId)).toBe(
      `/spaces/${ENCODED_FIXTURE}/dashboard`,
    );
    expect(spaceEntriesPath(spaceId)).toBe(
      `/spaces/${ENCODED_FIXTURE}/entries`,
    );
    expect(spaceEntryPath(spaceId, "entry/1")).toBe(
      `/spaces/${ENCODED_FIXTURE}/entries/entry%2F1`,
    );
    expect(spacePath(spaceId, "forms")).toBe(
      `/spaces/${ENCODED_FIXTURE}/forms`,
    );
    expect(spaceHistoryPath(spaceId)).toBe(
      `/spaces/${ENCODED_FIXTURE}/history`,
    );
  });
});
