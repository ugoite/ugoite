import { describe, expect, it } from "vitest";
import { entryDisplayLabel } from "./entry-label";
import type { EntryRecord } from "./types";

describe("entryDisplayLabel", () => {
  it("uses the generic label when an entry has no properties", () => {
    const entry = { id: "entry-1", properties: null } as unknown as EntryRecord;

    expect(entryDisplayLabel(entry)).toBe("Entry");
  });

  it("continues to prefer a human-authored title", () => {
    const entry = {
      id: "entry-1",
      properties: { Title: "  Planning notes  " },
    } as EntryRecord;

    expect(entryDisplayLabel(entry)).toBe("Planning notes");
  });
});
