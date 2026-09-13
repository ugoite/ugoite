import { describe, expect, it } from "vitest";
import {
  CreateEntryDraftSession,
  type CreateEntryDraftState,
} from "./create-entry-draft-session";

const state: CreateEntryDraftState = {
  title: "Meeting",
  fields: { Notes: "draft", "__asset:Notes": { asset_id: "asset-1" } },
  tags: ["work"],
  source: "# Meeting\n\n## Notes\ndraft",
  assetFields: { Notes: { asset_id: "asset-1" } },
  dirty: true,
};

describe("CreateEntryDraftSession", () => {
  it("keeps independent structured work for each Form and returns copies", () => {
    const session = new CreateEntryDraftSession();
    session.save("Meeting", state);
    session.save("Task", { ...state, title: "Task", dirty: false });

    const restored = session.restore("Meeting");
    expect(restored).toEqual(state);
    restored!.fields.Notes = "changed outside the session";
    expect(session.restore("Meeting")!.fields.Notes).toBe("draft");
    expect(session.restore("Task")!.title).toBe("Task");
    expect(session.hasDirtyWork()).toBe(true);
  });

  it("clears all work only on explicit discard", () => {
    const session = new CreateEntryDraftSession();
    session.save("Meeting", state);
    expect(session.hasDirtyWork()).toBe(true);
    session.clear();
    expect(session.restore("Meeting")).toBeUndefined();
    expect(session.hasDirtyWork()).toBe(false);
  });
});
