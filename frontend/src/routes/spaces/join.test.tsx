import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceInvitationJoinRoute from "./join";
import { authApi } from "~/lib/auth-api";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; children: unknown }) => (
    <a href={props.href}>{props.children}</a>
  ),
  useNavigate: () => vi.fn(),
}));

vi.mock("~/components/GlobalShell", () => ({
  GlobalShell: (props: { authenticated?: boolean; children: unknown }) => (
    <div data-authenticated={String(props.authenticated)}>{props.children}</div>
  ),
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: {
    acceptInvitation: vi.fn(),
    getSession: vi.fn(),
    listOidcProviders: vi.fn(),
    registerInvitation: vi.fn(),
  },
}));

describe("/spaces/join", () => {
  beforeEach(() => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: false });
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([]);
  });

  it("renders as a public shell before an invitation recipient signs in", async () => {
    render(() => <SpaceInvitationJoinRoute />);

    await waitFor(() =>
      expect(screen.getByText("Join a Space").parentElement?.parentElement)
        .toHaveAttribute("data-authenticated", "false")
    );
  });
});
