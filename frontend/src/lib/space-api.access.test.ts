import { beforeEach, describe, expect, it, vi } from "vitest";
import { spaceApi } from "./space-api";
import { protocolFetch } from "./ugoite-client/protocol";
import { publishCompositionAccessChange } from "./composition-access-changes";

vi.mock("./ugoite-client/protocol", () => ({
  protocolFetch: vi.fn(),
}));
vi.mock("./composition-access-changes", () => ({
  publishCompositionAccessChange: vi.fn(),
}));

describe("Space membership mutations", () => {
  beforeEach(() => vi.resetAllMocks());

  it("notifies after successful invites, role changes, and revocations", async () => {
    vi.mocked(protocolFetch)
      .mockResolvedValueOnce({ invitation_id: "invite-1" })
      .mockResolvedValueOnce({ principal_id: "member-1", role: "editor" })
      .mockResolvedValueOnce({ principal_id: "member-1", state: "revoked" });

    await spaceApi.inviteMember("space-1", {
      label: "Member",
      role: "editor",
    });
    await spaceApi.updateMemberRole("space-1", "member-1", {
      role: "editor",
    });
    await spaceApi.revokeMember("space-1", "member-1");

    expect(publishCompositionAccessChange).toHaveBeenCalledTimes(3);
    expect(publishCompositionAccessChange).toHaveBeenNthCalledWith(
      1,
      "space-1",
    );
    expect(publishCompositionAccessChange).toHaveBeenNthCalledWith(
      2,
      "space-1",
    );
    expect(publishCompositionAccessChange).toHaveBeenNthCalledWith(
      3,
      "space-1",
    );
  });

  it("does not notify when a membership mutation fails", async () => {
    vi.mocked(protocolFetch).mockRejectedValueOnce(new Error("denied"));

    await expect(spaceApi.revokeMember("space-1", "member-1"))
      .rejects.toThrow("denied");

    expect(publishCompositionAccessChange).not.toHaveBeenCalled();
  });
});
