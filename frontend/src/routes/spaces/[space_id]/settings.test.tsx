import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import SpaceSettingsRoute from "./settings";
import { setLocale } from "~/lib/i18n";
import { spaceApi } from "~/lib/ugoite-client";

vi.mock("@solidjs/router", () => ({
  useParams: () => ({ space_id: "space-1" }),
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
}));

vi.mock("~/components/SpaceShell", () => ({
  SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));

vi.mock("~/components/SpaceSettings", () => ({
  SpaceSettings: () => <div>Space settings</div>,
}));

vi.mock("~/lib/ugoite-client", () => ({
  spaceApi: {
    get: vi.fn(),
    listMembers: vi.fn(),
    patch: vi.fn(),
    testConnection: vi.fn(),
    inviteMember: vi.fn(),
    updateMemberRole: vi.fn(),
    revokeMember: vi.fn(),
  },
}));

const inviteResponse = (token: string, expiresAt: string) => ({
  invitation: {
    token,
    user_id: "user-1",
    role: "viewer",
    state: "pending",
    invited_by: "owner",
    invited_at: "2026-01-01T00:00:00Z",
    expires_at: expiresAt,
  },
  delivery: {},
  audit_event: {},
});

const formatExpiry = (value: string) =>
  new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));

describe("/spaces/:space_id/settings", () => {
  beforeEach(() => {
    setLocale("en");
    vi.resetAllMocks();
    (spaceApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "space-1",
      name: "Test Space",
      created_at: "2026-01-01T00:00:00Z",
    });
    (spaceApi.listMembers as ReturnType<typeof vi.fn>).mockResolvedValue([]);
  });

  it("shows invitation expiry and replaces stale invitation state", async () => {
    (spaceApi.inviteMember as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(inviteResponse("first-token", "2026-01-01T10:30:00Z"))
      .mockResolvedValueOnce(inviteResponse("second-token", "2026-01-02T09:15:00Z"));
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    render(() => <SpaceSettingsRoute />);

    await waitFor(() => expect(spaceApi.get).toHaveBeenCalled());

    fireEvent.input(screen.getByLabelText(/user id/i), {
      target: { value: "user-1" },
    });
    fireEvent.click(screen.getByRole("button", { name: /invite member/i }));

    await waitFor(() => {
      expect(screen.getByText("first-token")).toBeInTheDocument();
    });
    expect(screen.getByText(formatExpiry("2026-01-01T10:30:00Z")))
      .toBeInTheDocument();

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: /invite member/i }),
      ).not.toBeDisabled();
    });
    fireEvent.input(screen.getByLabelText(/user id/i), {
      target: { value: "user-2" },
    });
    fireEvent.click(screen.getByRole("button", { name: /invite member/i }));

    await waitFor(() => {
      expect(screen.getByText("second-token")).toBeInTheDocument();
    });
    expect(screen.queryByText("first-token")).not.toBeInTheDocument();
    expect(screen.getByText(formatExpiry("2026-01-02T09:15:00Z")))
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /copy token/i }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("second-token");
      expect(screen.getByRole("status")).toHaveTextContent(
        /copied to clipboard/i,
      );
    });
  });

  it("announces copy failure when the clipboard path is unavailable", async () => {
    (spaceApi.inviteMember as ReturnType<typeof vi.fn>).mockResolvedValue(
      inviteResponse("fallback-token", "2026-01-01T10:30:00Z"),
    );
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: vi.fn(() => false),
    });
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: undefined,
    });

    render(() => <SpaceSettingsRoute />);

    await waitFor(() => expect(spaceApi.get).toHaveBeenCalled());

    fireEvent.input(screen.getByLabelText(/user id/i), {
      target: { value: "user-1" },
    });
    fireEvent.click(screen.getByRole("button", { name: /invite member/i }));

    await waitFor(() => {
      expect(screen.getByText("fallback-token")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: /copy token/i }));

    await waitFor(() => {
      expect(
        screen.getByRole("alert"),
      ).toHaveTextContent(/copy to clipboard failed|clipboard is unavailable/i);
    });
  });
});
