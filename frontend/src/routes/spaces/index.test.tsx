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

const localDevAuthGuideUrl =
  "https://ugoite.github.io/ugoite/docs/guide/local-dev-auth-login";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigateMock,
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
    fireEvent.input(screen.getByLabelText("Space ID"), {
      target: { value: "my-space" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create space" }));

    await waitFor(() => {
      expect(spaceApi.create).toHaveBeenCalledWith("my-space");
      expect(navigateMock).toHaveBeenCalledWith("/spaces/my-space/dashboard");
    });
  });

  it("REQ-FE-002: labels the create-space field as a space ID and explains allowed characters", async () => {
    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Create space" }));

    expect(screen.getByLabelText("Space ID")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("e.g. team-notes")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Use letters, numbers, hyphens, or underscores. This becomes the space URL and storage ID.",
      ),
    ).toBeInTheDocument();
  });

  it("REQ-FE-002: rewrites invalid space_id backend errors into user-facing guidance", async () => {
    (spaceApi.create as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error(
        "Invalid space_id: My Space. Must be alphanumeric, hyphens, or underscores.",
      ),
    );

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Create space" }));
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
      new Error("403 Forbidden"),
    );

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("Failed to load spaces.")).toBeInTheDocument();
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
      new Error("401 Unauthorized"),
    );

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("Failed to load spaces.")).toBeInTheDocument();
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

  it("REQ-OPS-015: redirects unauthenticated users to the explicit login route", async () => {
    (spaceApi.list as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error("401 Unauthorized"),
    );

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith("/login?next=%2Fspaces");
    });
  });
});
