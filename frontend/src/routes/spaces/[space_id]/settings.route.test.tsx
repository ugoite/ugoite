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
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
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
      space_uid: "space-1",
      name: "Operations",
      created_at: "2026-01-01T00:00:00Z",
      storage_config: { uri: "file:///tmp/operations" },
    });
    vi.mocked(spaceApi.patch).mockResolvedValue({
      space_uid: "space-1",
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

  it("PR6: navigates settings as flat rows with History reaching space history", async () => {
    const { container } = render(() => <SpaceSettingsRoute />);
    await screen.findByRole("heading", { name: "General" });

    const nav = container.querySelector(
      'nav[aria-labelledby="settings-page-title"]',
    );
    expect(nav).not.toBeNull();
    expect(nav!.querySelector(".rowList")).not.toBeNull();
    expect(container.querySelector(".settingsNav")).toBeNull();
    expect(screen.getByRole("button", { name: "Members" })).toBeInTheDocument();
    const historyLink = screen.getByRole("link", { name: /History/ });
    expect(historyLink).toHaveAttribute("href", "/spaces/space-1/history");

    cleanup();
    searchParams.section = "history";
    render(() => <SpaceSettingsRoute />);
    expect(await screen.findByRole("heading", { name: "History" }))
      .toBeInTheDocument();
  });

  it("opens the settings navigation as a drawer and closes it on selection", async () => {
    const { container } = render(() => <SpaceSettingsRoute />);
    await screen.findByRole("heading", { name: "General" });

    // One category list only: desktop sidebar and mobile drawer share it.
    expect(
      container.querySelectorAll(
        'nav[aria-labelledby="settings-page-title"]',
      ),
    ).toHaveLength(1);
    const menuButton = screen.getByRole("button", {
      name: "Settings menu: General",
    });
    expect(menuButton).toHaveAttribute("aria-expanded", "false");
    expect(menuButton).toHaveAttribute("aria-controls", "settings-nav");
    expect(container.querySelector(".drawerBackdrop")).toBeNull();

    fireEvent.click(menuButton);
    expect(menuButton).toHaveAttribute("aria-expanded", "true");
    expect(container.querySelector(".drawerBackdrop")).not.toBeNull();
    // Focus moves into the drawer close control for keyboard users.
    const closeButton = container.querySelector(
      "#settings-nav button",
    ) as HTMLButtonElement;
    await waitFor(() => expect(document.activeElement).toBe(closeButton));

    fireEvent.click(screen.getByRole("button", { name: "Members" }));
    expect(setSearchParams).toHaveBeenCalledWith({ section: "members" });
    // Selecting a category closes the drawer and returns focus.
    expect(container.querySelector(".drawerBackdrop")).toBeNull();
    expect(menuButton).toHaveAttribute("aria-expanded", "false");
    expect(document.activeElement).toBe(menuButton);
  });

  it("closes the settings drawer on Escape", async () => {
    const { container } = render(() => <SpaceSettingsRoute />);
    await screen.findByRole("heading", { name: "General" });

    const menuButton = screen.getByRole("button", {
      name: "Settings menu: General",
    });
    fireEvent.click(menuButton);
    expect(container.querySelector(".drawerBackdrop")).not.toBeNull();

    const nav = container.querySelector("#settings-nav")!;
    fireEvent.keyDown(nav, { key: "Escape" });
    expect(container.querySelector(".drawerBackdrop")).toBeNull();
    expect(document.activeElement).toBe(menuButton);
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

  it("renders members with display names while exact IDs stay advanced-only", async () => {
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

    const nameCells = await screen.findAllByText("Alice Example");
    // Row + advanced disclosure.
    expect(nameCells).toHaveLength(2);
    const nameCell = nameCells[0];
    expect(nameCell).toHaveClass("membersPrimary");
    // No UUID list: rows never show raw principal IDs; exact IDs live in
    // the advanced disclosure only.
    for (const id of ["principal-1", "principal-2"]) {
      const node = screen.getByText(id);
      expect(node.closest("details")).not.toBeNull();
      expect(node.closest("tr")).toBeNull();
    }
    expect(container.querySelector(".membersSecondary")).toBeNull();
    expect(screen.getByText("invited")).toBeInTheDocument();
    // Exact IDs live in the advanced disclosure only.
    expect(screen.getByText("principal-2").closest("details")).not.toBeNull();

    const ownerRow = screen.getAllByText("Alice Example")[0].closest("tr")!;
    const ownerRole = ownerRow.querySelector("select")!;
    expect(ownerRole).toBeDisabled();
    expect(ownerRow.querySelector("button")).toBeDisabled();

    const editorRow = screen.getAllByText("Bob")[0].closest("tr")!;
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
    await screen.findAllByText("Bob");
    const row = screen.getAllByText("Bob")[0].closest("tr")!;

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

  it("shows a fallback name when the display name is missing with the ID advanced-only", async () => {
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
    const untitled = await screen.findAllByText("Untitled");
    // Row + advanced disclosure.
    expect(untitled).toHaveLength(2);

    // No duplicate: the row shows the fallback name, no secondary code.
    expect(container.querySelector(".membersSecondary")).toBeNull();
    // The exact ID stays available in the advanced disclosure.
    const idNode = screen.getByText("principal-9");
    expect(idNode.closest("details")).not.toBeNull();
    expect(idNode.closest("tr")).toBeNull();
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
