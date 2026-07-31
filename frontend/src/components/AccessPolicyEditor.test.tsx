import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { setLocale } from "~/lib/i18n";
import { accessApi } from "~/lib/access-api";
import { AccessPolicyEditor } from "./AccessPolicyEditor";

vi.mock("~/lib/access-api", () => ({
  accessApi: {
    get: vi.fn(),
    put: vi.fn(),
  },
}));

describe("AccessPolicyEditor", () => {
  beforeEach(() => {
    setLocale("ja");
    vi.mocked(accessApi.get).mockReset();
    vi.mocked(accessApi.put).mockReset();
  });

  it("does not allow saving an empty policy after loading failed", async () => {
    vi.mocked(accessApi.get)
      .mockRejectedValueOnce(new UgoiteApiError({
        kind: "internal",
        code: "INTERNAL_ERROR",
        status: 500,
        message: "backend unavailable",
        detail: { request_id: "req-1" },
      }))
      .mockResolvedValueOnce({
        policy_id: "policy-1",
        inherit_space_role: false,
        grants: [],
      });

    render(() => (
      <AccessPolicyEditor
        spaceId="space-1"
        kind="asset"
        resourceId="asset-1"
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "サーバーで問題が発生しました。",
    );
    expect(screen.getByRole("button", { name: "アクセス設定を保存" }))
      .toBeDisabled();
    expect(screen.getByPlaceholderText("プリンシパル UUID"))
      .toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "アクセス設定を保存" }));
    expect(accessApi.put).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "再試行" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "アクセス設定を保存" }))
        .toBeEnabled();
    });
  });

  it("treats a successful empty policy as editable", async () => {
    vi.mocked(accessApi.get).mockResolvedValue(null);
    vi.mocked(accessApi.put).mockResolvedValue({
      policy_id: "new-policy",
      inherit_space_role: true,
      grants: [],
    });

    render(() => (
      <AccessPolicyEditor
        spaceId="space-1"
        kind="entry"
        resourceId="entry-1"
      />
    ));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "アクセス設定を保存" }))
        .toBeEnabled();
    });
    fireEvent.click(screen.getByRole("button", { name: "アクセス設定を保存" }));
    await waitFor(() => expect(accessApi.put).toHaveBeenCalledOnce());
  });

  it("resets policy state when the reactive resource changes", async () => {
    const [resourceId, setResourceId] = createSignal("entry-a");
    vi.mocked(accessApi.get).mockImplementation(async (_spaceId, _kind, id) =>
      id === "entry-a"
        ? {
          policy_id: "policy-a",
          inherit_space_role: false,
          grants: [{ principal_id: "principal-a", actions: ["read"] }],
        }
        : {
          policy_id: "policy-b",
          inherit_space_role: true,
          grants: [],
        }
    );
    vi.mocked(accessApi.put).mockResolvedValue({
      policy_id: "policy-b",
      inherit_space_role: true,
      grants: [],
    });

    render(() => (
      <AccessPolicyEditor
        spaceId="space-1"
        kind="entry"
        resourceId={resourceId()}
      />
    ));

    expect(await screen.findByText(/principal-a/)).toBeInTheDocument();
    setResourceId("entry-b");

    await waitFor(() => {
      expect(screen.queryByText(/principal-a/)).toBeNull();
      expect(screen.getByRole("button", { name: "アクセス設定を保存" }))
        .toBeEnabled();
    });

    fireEvent.click(screen.getByRole("button", { name: "アクセス設定を保存" }));
    await waitFor(() => {
      expect(accessApi.put).toHaveBeenLastCalledWith(
        "space-1",
        "entry",
        "entry-b",
        {
          policy_id: "policy-b",
          inherit_space_role: true,
          grants: [],
        },
      );
    });
  });
});
