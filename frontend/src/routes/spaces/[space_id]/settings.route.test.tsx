import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceSettingsRoute from "./settings";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { setLocale } from "~/lib/i18n";
import { spaceApi } from "~/lib/ugoite-client";

const searchParams: Record<string, string> = {};
const setSearchParams = vi.fn();

vi.mock("@solidjs/router", () => ({
  useParams: () => ({ space_id: "space-1" }),
  useSearchParams: () => [searchParams, setSearchParams],
}));

vi.mock("~/components/SpaceShell", () => ({
  SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));

vi.mock("~/routes/settings/security", () => ({
  CredentialSettings: () => <div>Credentials route</div>,
}));

vi.mock("~/components/AuditLogViewer", () => ({
  SpaceAuditLogViewer: () => <div>Audit viewer</div>,
}));

vi.mock("~/lib/ugoite-client", () => ({
  spaceApi: {
    get: vi.fn(),
    patch: vi.fn(),
    testConnection: vi.fn(),
    listMembers: vi.fn(),
    listAgents: vi.fn(),
    listAudit: vi.fn(),
    inviteMember: vi.fn(),
    updateMemberRole: vi.fn(),
    revokeMember: vi.fn(),
    createAgent: vi.fn(),
    revokeAgent: vi.fn(),
  },
}));

describe("SpaceSettingsRoute", () => {
  beforeEach(() => {
    setLocale("en");
    for (const key of Object.keys(searchParams)) delete searchParams[key];
    setSearchParams.mockReset();
    vi.mocked(spaceApi.get).mockResolvedValue({
      id: "space-1",
      name: "Operations",
      created_at: "2026-01-01T00:00:00Z",
      storage_config: { uri: "file:///tmp/operations" },
    });
    vi.mocked(spaceApi.patch).mockResolvedValue({
      id: "space-1",
      name: "Operations",
      created_at: "2026-01-01T00:00:00Z",
    });
    vi.mocked(spaceApi.testConnection).mockResolvedValue({ status: "ok" });
    vi.mocked(spaceApi.listMembers).mockResolvedValue([]);
    vi.mocked(spaceApi.listAgents).mockResolvedValue([]);
    vi.mocked(spaceApi.listAudit).mockResolvedValue({
      items: [],
      total: 0,
      offset: 0,
      limit: 25,
    });
    vi.mocked(spaceApi.createAgent).mockReset();
  });

  it("renders the general, language, and storage route surfaces", async () => {
    render(() => <SpaceSettingsRoute />);

    expect(screen.getByRole("heading", { name: "Settings" }))
      .toHaveClass("ui-sr-only");
    expect(await screen.findByRole("heading", { name: "General" }))
      .toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Language" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Members" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Agents" })).toBeNull();
    expect(screen.getByRole("button", { name: "Credentials" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Storage" })).toBeInTheDocument();

    cleanup();
    searchParams.section = "storage";
    render(() => <SpaceSettingsRoute />);
    expect(await screen.findByRole("heading", { name: "Storage" }))
      .toBeInTheDocument();

    cleanup();
    searchParams.section = "audit";
    render(() => <SpaceSettingsRoute />);
    expect(await screen.findByRole("heading", { name: "Audit Log" }))
      .toBeInTheDocument();
    expect(screen.getByText("Audit viewer")).toBeInTheDocument();
  });

  it("keeps protocol role tokens visible on the route", async () => {
    searchParams.section = "members";
    vi.mocked(spaceApi.listMembers).mockResolvedValue([{
      principal: {
        principal_id: "principal-1",
        display_name: "Alice",
        kind: "user",
        state: "active",
      },
      role: "owner",
    }]);
    render(() => <SpaceSettingsRoute />);
    expect(await screen.findByRole("option", { name: /owner.*Owner/ }))
      .toBeInTheDocument();

    expect(screen.queryByText("No agents found.")).toBeNull();
  });

  it("renders members as an audit-style table with readable principal detail", async () => {
    searchParams.section = "members";
    vi.mocked(spaceApi.listMembers).mockResolvedValue([
      {
        principal: {
          principal_id: "principal-1",
          display_name: "Alice Example",
          kind: "human",
          state: "active",
          created_at: "2026-01-01T00:00:00Z",
        },
        role: "owner",
        created_at: "2026-01-01T00:00:00Z",
      },
      {
        principal: {
          principal_id: "principal-2",
          display_name: "Bob",
          kind: "human",
          state: "invited",
          created_at: "2026-01-02T00:00:00Z",
        },
        role: "editor",
        created_at: "2026-01-02T00:00:00Z",
      },
    ]);
    const { container } = render(() => <SpaceSettingsRoute />);

    for (const name of ["Member", "Role", "State", "Actions"]) {
      expect(await screen.findByRole("columnheader", { name }))
        .toBeInTheDocument();
    }
    expect(container.querySelector(".ui-table-wrapper .ui-table.membersTable"))
      .toBeInTheDocument();
    expect(container.querySelector(".rowStack")).toBeNull();

    const nameCell = screen.getByText("Alice Example");
    expect(nameCell).toHaveClass("membersPrimary");
    expect(nameCell.closest("td")).toHaveAttribute("title", "Alice Example");

    const idCell = screen.getByText("principal-2").closest("td");
    expect(idCell).toHaveAttribute("title", "Bob");
    expect(screen.getByText("principal-2")).toHaveClass("membersSecondary");
    expect(screen.getByText("invited")).toBeInTheDocument();

    const ownerRow = screen.getByText("Alice Example").closest("tr")!;
    const ownerRole = ownerRow.querySelector("select")!;
    expect(ownerRole).toBeDisabled();
    expect(ownerRow.querySelector("button")).toBeDisabled();

    const editorRow = screen.getByText("Bob").closest("tr")!;
    expect(editorRow.querySelector("select")).not.toBeDisabled();
    expect(editorRow.querySelector("button")).not.toBeDisabled();
  });

  it("keeps role updates and revokes working from the members table", async () => {
    searchParams.section = "members";
    vi.mocked(spaceApi.listMembers).mockResolvedValue([{
      principal: {
        principal_id: "principal-2",
        display_name: "Bob",
        kind: "human",
        state: "active",
        created_at: "2026-01-02T00:00:00Z",
      },
      role: "editor",
      created_at: "2026-01-02T00:00:00Z",
    }]);
    vi.mocked(spaceApi.updateMemberRole).mockResolvedValue({
      principal_id: "principal-2",
      role: "viewer",
    });
    vi.mocked(spaceApi.revokeMember).mockResolvedValue({
      principal_id: "principal-2",
      state: "revoked",
    });
    render(() => <SpaceSettingsRoute />);
    await screen.findByText("Bob");
    const row = screen.getByText("Bob").closest("tr")!;

    fireEvent.change(row.querySelector("select")!, {
      target: { value: "viewer" },
    });
    await waitFor(() => {
      expect(spaceApi.updateMemberRole).toHaveBeenCalledWith(
        "space-1",
        "principal-2",
        { role: "viewer" },
      );
    });

    fireEvent.click(row.querySelector("button")!);
    await waitFor(() => {
      expect(spaceApi.revokeMember).toHaveBeenCalledWith(
        "space-1",
        "principal-2",
      );
    });
  });

  it("shows the principal ID once when the display name is missing", async () => {
    searchParams.section = "members";
    vi.mocked(spaceApi.listMembers).mockResolvedValue([{
      principal: {
        principal_id: "principal-9",
        display_name: "",
        kind: "human",
        state: "active",
        created_at: "2026-01-03T00:00:00Z",
      },
      role: "viewer",
      created_at: "2026-01-03T00:00:00Z",
    }]);
    const { container } = render(() => <SpaceSettingsRoute />);
    await screen.findByText("principal-9");

    const cell = screen.getByText("principal-9").closest("td")!;
    expect(cell).toHaveClass("membersNameCell");
    // No duplicate: the ID appears once as primary, no secondary code.
    expect(cell.querySelectorAll("code").length).toBe(0);
    expect(container.querySelector(".membersSecondary")).toBeNull();
  });

  it("localizes the Member heading in Japanese", async () => {
    setLocale("ja");
    searchParams.section = "members";
    vi.mocked(spaceApi.listMembers).mockResolvedValue([{
      principal: {
        principal_id: "principal-1",
        display_name: "Alice",
        kind: "human",
        state: "active",
        created_at: "2026-01-01T00:00:00Z",
      },
      role: "owner",
      created_at: "2026-01-01T00:00:00Z",
    }]);
    render(() => <SpaceSettingsRoute />);
    expect(await screen.findByRole("columnheader", { name: "メンバー" }))
      .toBeInTheDocument();
  });

  it("renders localized known errors with unknown details for a section route", async () => {
    setLocale("ja");
    searchParams.section = "members";
    vi.mocked(spaceApi.listMembers).mockRejectedValue(
      new UgoiteApiError({
        kind: "forbidden",
        code: "FORBIDDEN",
        status: 403,
        message: "forbidden",
        detail: { request_id: "members-1" },
      }),
    );
    render(() => <SpaceSettingsRoute />);
    await waitFor(() => {
      expect(screen.getByText(/権限がありません/)).toBeInTheDocument();
    });
    expect(screen.getByText(/members-1/)).toBeInTheDocument();
  });

  it("falls back from the future agents section to general settings", async () => {
    searchParams.section = "agents";
    render(() => <SpaceSettingsRoute />);

    expect(await screen.findByRole("heading", { name: "General" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Agents" })).toBeNull();
  });
});
