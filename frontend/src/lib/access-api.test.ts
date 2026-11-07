import { beforeEach, describe, expect, it, vi } from "vitest";
import { accessApi } from "./access-api";
import { protocolFetch } from "./ugoite-client/protocol";
import { publishCompositionAccessChange } from "./composition-access-changes";

vi.mock("./ugoite-client/protocol", () => ({
  protocolFetch: vi.fn(),
}));
vi.mock("./composition-access-changes", () => ({
  publishCompositionAccessChange: vi.fn(),
}));

describe("resource access mutations", () => {
  beforeEach(() => vi.resetAllMocks());

  it("notifies Composition views after a successful resource policy update", async () => {
    const policy = {
      policy_id: "policy-1",
      inherit_space_role: true,
      grants: [],
    };
    vi.mocked(protocolFetch).mockResolvedValueOnce(policy);

    await expect(accessApi.put("space-1", "form", "form-1", policy))
      .resolves.toEqual(policy);

    expect(publishCompositionAccessChange).toHaveBeenCalledOnce();
    expect(publishCompositionAccessChange).toHaveBeenCalledWith("space-1");
  });

  it("does not notify after a rejected policy update", async () => {
    vi.mocked(protocolFetch).mockRejectedValueOnce(new Error("denied"));

    await expect(
      accessApi.put("space-1", "form", "form-1", {
        policy_id: "policy-1",
        inherit_space_role: true,
        grants: [],
      }),
    ).rejects.toThrow("denied");

    expect(publishCompositionAccessChange).not.toHaveBeenCalled();
  });
});
