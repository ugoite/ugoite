import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import DeviceApprovalRoute from "./device";
import { spaceApi } from "~/lib/ugoite-client";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";

vi.mock("@solidjs/router", () => ({
  useSearchParams: () => [{ user_code: "ABCD" }],
}));

vi.mock("~/lib/ugoite-client", () => ({
  spaceApi: {
    list: vi.fn(),
  },
}));

describe("/device", () => {
  beforeEach(() => {
    vi.mocked(spaceApi.list).mockReset();
  });

  it("keeps authenticated permission errors distinguishable", async () => {
    vi.mocked(spaceApi.list).mockRejectedValue(
      new UgoiteApiError({
        kind: "forbidden",
        code: "FORBIDDEN",
        status: 403,
        message: "forbidden",
        detail: { request_id: "device-space-list-1" },
      }),
    );

    render(() => <DeviceApprovalRoute />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("You do not have permission to do that.");
    expect(alert).toHaveTextContent("device-space-list-1");
  });
});
