import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { setLocale } from "~/lib/i18n";
import { accessApi } from "~/lib/access-api";
import { spaceApi } from "~/lib/ugoite-client";
import { AccessPolicyEditor } from "./AccessPolicyEditor";

vi.mock("~/lib/access-api", () => ({
  accessApi: {
    get: vi.fn(),
    put: vi.fn(),
  },
}));

vi.mock("~/lib/ugoite-client", () => ({
  spaceApi: {
    listMembers: vi.fn(),
  },
}));

const member = (
  id: string,
  name: string,
  state: "invited" | "active" | "suspended" | "revoked" = "active",
) => ({
  principal: {
    principal_id: id,
    kind: "human" as const,
    display_name: name,
    state,
    created_at: "2025-01-01T00:00:00Z",
  },
  role: "editor" as const,
  created_at: "2025-01-01T00:00:00Z",
});

describe("AccessPolicyEditor", () => {
  beforeEach(() => {
    setLocale("ja");
    vi.mocked(accessApi.get).mockReset();
    vi.mocked(accessApi.put).mockReset();
    vi.mocked(spaceApi.listMembers).mockReset().mockResolvedValue([]);
  });

  it("does not allow saving an empty policy after loading failed", async () => {
    vi.mocked(accessApi.get)
      .mockRejectedValueOnce(
        new UgoiteApiError({
          kind: "internal",
          code: "INTERNAL_ERROR",
          status: 500,
          message: "backend unavailable",
          detail: { request_id: "req-1" },
        }),
      )
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
    expect(screen.getByRole("combobox", { name: "メンバー" }))
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
    vi.mocked(spaceApi.listMembers).mockResolvedValue([
      member("principal-a", "Ada Example"),
    ]);

    render(() => (
      <AccessPolicyEditor
        spaceId="space-1"
        kind="entry"
        resourceId={resourceId()}
      />
    ));

    expect(await screen.findAllByText(/Ada Example/)).toHaveLength(2);
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

  it("REQ-UX-ACCESS-001: uses member names in grants and keeps unresolved IDs in collapsed details", async () => {
    const unknownPrincipalId = "principal-unavailable-001";
    vi.mocked(spaceApi.listMembers).mockResolvedValue([
      member("principal-a", "Ada Example"),
    ]);
    vi.mocked(accessApi.get).mockResolvedValue({
      policy_id: "policy-1",
      inherit_space_role: true,
      grants: [
        { principal_id: "principal-a", actions: ["read"] },
        { principal_id: unknownPrincipalId, actions: ["update"] },
      ],
    });

    render(() => (
      <AccessPolicyEditor
        spaceId="space-1"
        kind="entry"
        resourceId="entry-1"
      />
    ));

    expect(await screen.findAllByText("Ada Example")).toHaveLength(2);
    expect(screen.getByText("不明なメンバー")).toBeInTheDocument();
    expect(screen.getByText("principal-a")).not.toBeVisible();
    expect(screen.getByText(unknownPrincipalId)).not.toBeVisible();

    const detailSummaries = screen.getAllByText("詳細情報");
    fireEvent.click(detailSummaries[1]);
    expect(screen.getByText(unknownPrincipalId)).toBeInTheDocument();
  });

  it("REQ-UX-ACCESS-001: selects a member by name and preserves its ID in the grant payload", async () => {
    const principalId = "principal-a";
    vi.mocked(spaceApi.listMembers).mockResolvedValue([
      member(principalId, "Ada Example"),
      member("principal-suspended", "Suspended Member", "suspended"),
    ]);
    vi.mocked(accessApi.get).mockResolvedValue(null);
    vi.mocked(accessApi.put).mockResolvedValue({
      policy_id: "policy-1",
      inherit_space_role: true,
      grants: [{ principal_id: principalId, actions: ["read"] }],
    });

    render(() => (
      <AccessPolicyEditor
        spaceId="space-1"
        kind="entry"
        resourceId="entry-1"
      />
    ));

    const principalSelect = await screen.findByRole("combobox", {
      name: "メンバー",
    });
    await waitFor(() => expect(principalSelect).toBeEnabled());
    expect(principalSelect).toHaveTextContent("Ada Example");
    expect(principalSelect).not.toHaveTextContent(principalId);
    expect(principalSelect).not.toHaveTextContent("Suspended Member");

    fireEvent.change(principalSelect, { target: { value: principalId } });
    fireEvent.click(screen.getByRole("button", { name: "権限を追加" }));
    expect(await screen.findAllByText("Ada Example")).toHaveLength(2);
    expect(screen.getByText(principalId)).not.toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "アクセス設定を保存" }));
    await waitFor(() => {
      expect(accessApi.put).toHaveBeenCalledWith(
        "space-1",
        "entry",
        "entry-1",
        expect.objectContaining({
          grants: [{ principal_id: principalId, actions: ["read"] }],
        }),
      );
    });
  });

  it("REQ-UX-ACCESS-001: shows a neutral label and blocks member selection when the directory fails", async () => {
    const unresolvedPrincipalId = "principal-unavailable-001";
    vi.mocked(spaceApi.listMembers)
      .mockRejectedValueOnce(new Error("directory unavailable"))
      .mockResolvedValueOnce([member("principal-a", "Ada Example")]);
    vi.mocked(accessApi.get).mockResolvedValue({
      policy_id: "policy-1",
      inherit_space_role: true,
      grants: [{ principal_id: unresolvedPrincipalId, actions: ["read"] }],
    });

    render(() => (
      <AccessPolicyEditor
        spaceId="space-1"
        kind="entry"
        resourceId="entry-1"
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "メンバーを読み込めませんでした。",
    );
    expect(screen.getByText("不明なメンバー")).toBeInTheDocument();
    expect(screen.getByText(unresolvedPrincipalId)).not.toBeVisible();
    expect(screen.getByRole("combobox", { name: "メンバー" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "再試行" }));
    await waitFor(() => {
      expect(screen.getByRole("combobox", { name: "メンバー" }))
        .toBeEnabled();
    });
  });
});
