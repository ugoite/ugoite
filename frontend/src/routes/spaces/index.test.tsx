import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import SpacesIndexRoute from "./index";
import { authApi, spaceApi } from "~/lib/ugoite-client";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";

const localDevAuthGuideUrl =
  "https://ugoite.github.io/ugoite/docs/guide/develop/local-dev-auth-login";
const browserWalkthroughUrl =
  "https://ugoite.github.io/ugoite/docs/guide/start/browser-first-entry";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigateMock,
  useParams: () => ({}),
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
}));

vi.mock("~/lib/ugoite-client", () => ({
  authApi: {
    loginWithPasskey: vi.fn(),
  },
  spaceApi: {
    list: vi.fn(),
    create: vi.fn(),
  },
}));

describe("/spaces", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    (spaceApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);
    (spaceApi.create as ReturnType<typeof vi.fn>).mockReset();
    (authApi.loginWithPasskey as ReturnType<typeof vi.fn>).mockReset();
  });

  it("REQ-FE-002: shows a create-space action when no spaces exist", async () => {
    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    expect(screen.getByRole("button", { name: "Create space" }))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Join with invitation" }))
      .toHaveAttribute(
        "href",
        "/spaces/join",
      );
    expect(
      screen.getByRole("link", {
        name: "Learn how to create your first entry in the browser",
      }),
    ).toHaveAttribute("href", browserWalkthroughUrl);
    expect(spaceApi.create).not.toHaveBeenCalled();
  });

  it("REQ-FE-002: creates a space only after explicit user submission", async () => {
    (spaceApi.create as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "my-space",
      name: "my-space",
    });

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Create space" }));
    fireEvent.input(screen.getByLabelText("Space name"), {
      target: { value: "プロジェクトメモ 📝" },
    });
    fireEvent.input(screen.getByLabelText("Space ID"), {
      target: { value: "my-space" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create space" }));

    await waitFor(() => {
      expect(spaceApi.create).toHaveBeenCalledWith({
        name: "プロジェクトメモ 📝",
        slug: "my-space",
      });
      expect(navigateMock).toHaveBeenCalledWith("/spaces/my-space/dashboard");
    });
  });

  it("REQ-FE-002: labels the create-space field as a space ID and explains allowed characters", async () => {
    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Create space" }));

    expect(screen.getByLabelText("Space name")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("e.g. Project notes"))
      .toBeInTheDocument();
    expect(screen.getByLabelText("Space ID")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("e.g. team-notes")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Use letters, numbers, hyphens, or underscores. This is the stable URL and storage identifier.",
      ),
    ).toBeInTheDocument();
  });

  it("REQ-FE-002: rewrites invalid space_id backend errors into user-facing guidance", async () => {
    (spaceApi.create as ReturnType<typeof vi.fn>).mockRejectedValue(
      new UgoiteApiError({
        kind: "invalid_arguments",
        code: "INVALID_IDENTIFIER",
        status: 400,
        message: "Invalid space_id",
      }),
    );

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Create space" }));
    fireEvent.input(screen.getByLabelText("Space name"), {
      target: { value: "My space" },
    });
    fireEvent.input(screen.getByLabelText("Space ID"), {
      target: { value: "My Space" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create space" }));

    await waitFor(() => {
      expect(
        screen.getByText(
          "Space IDs can use only letters, numbers, hyphens, and underscores.",
        ),
      ).toBeInTheDocument();
    });
  });

  it("reauthenticates with Passkey when Space creation needs recent assurance", async () => {
    (spaceApi.create as ReturnType<typeof vi.fn>)
      .mockRejectedValueOnce(
        Object.assign(
          new Error("Failed to create space: repeat Passkey authentication"),
          { code: "RECENT_PASSKEY_REQUIRED" },
        ),
      )
      .mockResolvedValueOnce({ id: "my-space", name: "my-space" });
    (authApi.loginWithPasskey as ReturnType<typeof vi.fn>).mockResolvedValue(
      undefined,
    );

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Create space" }));
    fireEvent.input(screen.getByLabelText("Space name"), {
      target: { value: "My space" },
    });
    fireEvent.input(screen.getByLabelText("Space ID"), {
      target: { value: "my-space" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create space" }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Authenticate with Passkey" }))
        .toBeInTheDocument();
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Authenticate with Passkey" }),
    );

    await waitFor(() => {
      expect(authApi.loginWithPasskey).toHaveBeenCalledOnce();
      expect(spaceApi.create).toHaveBeenCalledTimes(2);
      expect(navigateMock).toHaveBeenCalledWith("/spaces/my-space/dashboard");
    });
  });

  it("REQ-FE-056: does not show persistent auth guidance during normal space listing", async () => {
    (spaceApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([
      { id: "default", name: "default" },
    ]);

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("Available Spaces")).toBeInTheDocument();
      expect(screen.getByText("Open Space")).toBeInTheDocument();
    });

    expect(screen.queryByRole("heading", { name: "Authentication" })).not
      .toBeInTheDocument();
    expect(
      screen.queryByText(
        /localhost and remote mode both require authentication/i,
      ),
    ).not.toBeInTheDocument();
  });

  it("REQ-FE-001: lists every authorized Space in one section", async () => {
    (spaceApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        id: "operations",
        name: "Operations",
        created_at: "2025-01-01T00:00:00Z",
      },
      {
        id: "default",
        name: "default",
        created_at: "2025-01-01T00:00:00Z",
      },
    ]);

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByRole("list", { name: "Spaces" }))
        .toBeInTheDocument();
    });

    const spacesList = screen.getByRole("list", { name: "Spaces" });
    expect(within(spacesList).getByText("default")).toBeInTheDocument();
    expect(within(spacesList).getByText("Operations")).toBeInTheDocument();
    expect(within(spacesList).getAllByRole("link", { name: "Open Space" })[0])
      .toHaveAttribute(
        "href",
        "/spaces/default/dashboard",
      );
  });

  it("REQ-FE-002: treats any authorized Space as selectable content", async () => {
    (spaceApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        id: "operations",
        name: "Operations",
        created_at: "2025-01-01T00:00:00Z",
      },
    ]);

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByRole("list", { name: "Spaces" }))
        .toBeInTheDocument();
    });
    expect(screen.queryByText("No spaces available.")).not.toBeInTheDocument();
  });

  it("REQ-FE-056: shows concise auth errors only when space loading fails", async () => {
    (spaceApi.list as ReturnType<typeof vi.fn>).mockRejectedValue(
      new UgoiteApiError({
        kind: "forbidden",
        code: "FORBIDDEN",
        status: 403,
        message: "Forbidden",
      }),
    );

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(
        screen.getByText(
          "You do not have permission to do that. (Details: Forbidden)",
        ),
      ).toBeInTheDocument();
    });

    expect(
      screen.getByText(
        "You are signed in but do not have permission to view these spaces.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(
        /localhost and remote mode both require authentication/i,
      ),
    ).not.toBeInTheDocument();
  });

  it("REQ-FE-056: links auth guidance to Local Dev Auth/Login when auth fails", async () => {
    (spaceApi.list as ReturnType<typeof vi.fn>).mockRejectedValue(
      new UgoiteApiError({
        kind: "forbidden",
        code: "AUTHENTICATION_FAILED",
        status: 401,
        message: "Unauthorized",
      }),
    );

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(
        screen.getByText("Authentication failed. (Details: Unauthorized)"),
      ).toBeInTheDocument();
    });

    expect(
      screen.getByText(
        "Authentication required. Open /login to start a local browser session.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Local Dev Auth/Login" }))
      .toHaveAttribute(
        "href",
        localDevAuthGuideUrl,
      );
  });
});
