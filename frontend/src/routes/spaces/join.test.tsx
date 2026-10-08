import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceInvitationJoinRoute from "./join";
import { authApi } from "~/lib/auth-api";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { readFileSync } from "node:fs";
import { parse } from "yaml";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; children: unknown }) => (
    <a href={props.href}>{props.children}</a>
  ),
  useNavigate: () => navigateMock,
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
    loginWithOidc: vi.fn(),
    registerInvitation: vi.fn(),
  },
  oidcIssuerLabel: (issuer: string) => {
    const url = new URL(issuer);
    return `${url.host}${url.pathname === "/" ? "" : url.pathname}`;
  },
}));

describe("/spaces/join", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    vi.mocked(authApi.acceptInvitation).mockReset();
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: false });
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([]);
    vi.mocked(authApi.loginWithOidc).mockReset();
    vi.mocked(authApi.registerInvitation).mockReset();
  });

  it("REQ-UX-NAV-001: states the join context once without a competing title", async () => {
    render(() => <SpaceInvitationJoinRoute />);

    expect(screen.getAllByRole("heading")).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Join" })).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: "Join" }).parentElement
          ?.parentElement?.parentElement,
      ).toHaveAttribute("data-authenticated", "false")
    );
  });

  it("REQ-FE-040: matches the invitation Join declaration to the rendered route", () => {
    const contract = parse(
      readFileSync(
        "../docs/spec/ui/components/invitation-join.yaml",
        "utf8",
      ),
    ) as {
      component_group?: Record<string, unknown>;
      components?: Array<Record<string, unknown>>;
    };
    const components = contract.components ?? [];
    const header = components.find((component) =>
      component.id === "join-context"
    );
    const form = components.find((component) =>
      component.id === "invitation-acceptance"
    );
    const identityOptions = components.find((component) =>
      component.id === "identity-options"
    );
    const failure = components.find((component) =>
      component.id === "invitation-failure"
    );

    expect(contract.component_group).toMatchObject({
      id: "invitation-join",
      routes: ["/spaces/join"],
    });
    expect(header).toMatchObject({
      type: "page-header",
      title: "Join",
      visible_headings: 1,
    });
    expect(form).toMatchObject({
      type: "form",
      invitation_token: {
        type: "required-multiline-text-input",
        label: "Invitation token",
      },
      accept_action: {
        type: "submit-button",
        label: "Accept invitation",
      },
    });

    render(() => <SpaceInvitationJoinRoute />);
    expect(screen.getAllByRole("heading")).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Join" })).toBeInTheDocument();
    expect(
      screen.getByLabelText("Invitation token"),
    ).toBeRequired();
    expect(
      screen.getByRole("button", { name: "Accept invitation" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("link", { name: "Already registered? Sign in" }),
    ).toHaveAttribute("href", "/login");
    expect(identityOptions).toMatchObject({
      oidc_providers: {
        label: "Continue with issuer-host-and-nonroot-path",
        accessible_name_uses_provider_id: false,
      },
    });
    expect(failure).toMatchObject({
      type: "alert",
      role: "alert",
      includes: ["failure-message", "resume-guidance"],
      show_spaces_action_when:
        "invitation-is-consumed-and-visitor-remains-unauthenticated",
    });
  });

  it("accepts an invitation for an already signed-in account", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    vi.mocked(authApi.acceptInvitation).mockResolvedValue();
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Accept invitation" }));

    await waitFor(() => {
      expect(authApi.acceptInvitation).toHaveBeenCalledWith("invitation-token");
      expect(navigateMock).toHaveBeenCalledWith("/spaces", { replace: true });
    });
    expect(authApi.registerInvitation).not.toHaveBeenCalled();
  });

  it("starts OIDC login with the invitation token", async () => {
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([{
      provider_id: "provider-1",
      issuer: "https://issuer.example/team",
      client_id: "client",
    }]);
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Continue with issuer.example/team",
      }),
    );

    expect(authApi.loginWithOidc).toHaveBeenCalledWith(
      "provider-1",
      "invitation-token",
    );
    expect(screen.queryByText("provider-1")).not.toBeInTheDocument();
  });

  it("shows the expiry reason with resume guidance and stays on join", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    vi.mocked(authApi.acceptInvitation).mockRejectedValue(
      new UgoiteApiError({
        kind: "expired",
        message: "Invitation has expired",
        code: "INVITATION_EXPIRED",
        status: 410,
      }),
    );
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Accept invitation" }));

    await screen.findByRole("alert");
    expect(
      screen.getByText("The invitation has expired.", { exact: false }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Ask the Space owner for a new invitation and open the new link.",
      ),
    ).toBeInTheDocument();
    expect(navigateMock).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("link", { name: "Go to Spaces" }),
    ).not.toBeInTheDocument();
  });

  it("returns an authenticated visitor to Spaces when a consumed invitation is reopened", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    vi.mocked(authApi.acceptInvitation).mockRejectedValue(
      new UgoiteApiError({
        kind: "conflict",
        message: "Invitation is no longer pending",
        code: "INVITATION_NOT_PENDING",
        status: 409,
      }),
    );
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Accept invitation" }));

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith("/spaces", { replace: true });
    });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    // Session is re-checked after NOT_PENDING instead of trusting the
    // submit-start snapshot alone.
    expect(vi.mocked(authApi.getSession).mock.calls.length).toBeGreaterThan(1);
  });

  it("issues at most one activation request while a submit is pending", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    vi.mocked(authApi.acceptInvitation).mockImplementation(() => gate);
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    const submit = screen.getByRole("button", { name: "Accept invitation" });
    fireEvent.click(submit);
    fireEvent.click(submit);

    await waitFor(() => {
      expect(authApi.acceptInvitation).toHaveBeenCalledTimes(1);
    });
    expect(submit).toBeDisabled();
    release();
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith("/spaces", { replace: true });
    });
    expect(authApi.acceptInvitation).toHaveBeenCalledTimes(1);
  });

  it("lands on Spaces when a concurrent activation consumed the invitation", async () => {
    let consumed = false;
    vi.mocked(authApi.getSession).mockImplementation(async () => ({
      authenticated: consumed,
    }));
    vi.mocked(authApi.registerInvitation).mockImplementation(async () => {
      consumed = true;
      throw new UgoiteApiError({
        kind: "conflict",
        message: "Invitation is no longer pending",
        code: "INVITATION_NOT_PENDING",
        status: 409,
      });
    });
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Accept invitation" }));

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith("/spaces", { replace: true });
    });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("stays fail-closed when NOT_PENDING and the visitor is still unauthenticated", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: false });
    vi.mocked(authApi.registerInvitation).mockRejectedValue(
      new UgoiteApiError({
        kind: "conflict",
        message: "Invitation is no longer pending",
        code: "INVITATION_NOT_PENDING",
        status: 409,
      }),
    );
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Accept invitation" }));

    await screen.findByRole("alert");
    expect(navigateMock).not.toHaveBeenCalled();
    expect(
      screen.getByRole("link", { name: "Go to Spaces" }),
    ).toBeInTheDocument();
  });

  it("strips the invitation token from the URL hash on success", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    vi.mocked(authApi.acceptInvitation).mockResolvedValue();
    const replaceState = vi.spyOn(history, "replaceState");
    window.location.hash = "#token=invitation-token";
    try {
      render(() => <SpaceInvitationJoinRoute />);
      expect(screen.getByLabelText("Invitation token")).toHaveValue(
        "invitation-token",
      );

      fireEvent.click(
        screen.getByRole("button", { name: "Accept invitation" }),
      );

      await waitFor(() => {
        expect(navigateMock).toHaveBeenCalledWith("/spaces", {
          replace: true,
        });
      });
      expect(replaceState).toHaveBeenCalledWith(
        null,
        "",
        window.location.pathname,
      );
      expect(window.location.hash).toBe("");
    } finally {
      replaceState.mockRestore();
      window.location.hash = "";
    }
  });

  it("shows invalid invitations with resume guidance", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    vi.mocked(authApi.acceptInvitation).mockRejectedValue(
      new UgoiteApiError({
        kind: "not_found",
        message: "Invitation was not found",
        code: "INVITATION_NOT_FOUND",
        status: 404,
      }),
    );
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Accept invitation" }));

    await screen.findByRole("alert");
    expect(
      screen.getByText("The invitation was not found.", { exact: false }),
    ).toBeInTheDocument();
    expect(navigateMock).not.toHaveBeenCalled();
  });
});
