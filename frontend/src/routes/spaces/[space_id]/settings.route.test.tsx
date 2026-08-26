import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen, waitFor } from "@solidjs/testing-library";
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
