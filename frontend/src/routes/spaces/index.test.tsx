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
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    ["aria-label"]?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
    >
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

    expect(screen.getByRole("button", { name: "+ Space" }))
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
      id: "legacy-space-id",
      space_uid: "019f1234-5678-7abc-8def-0123456789ab",
      name: "my-space",
    });

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "+ Space" }));
    fireEvent.input(screen.getByLabelText("Space name"), {
      target: { value: "プロジェクトメモ 📝" },
    });
    fireEvent.input(screen.getByLabelText("Space slug"), {
      target: { value: "my-space" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create space" }));

    await waitFor(() => {
      expect(spaceApi.create).toHaveBeenCalledWith({
        name: "プロジェクトメモ 📝",
        slug: "my-space",
      });
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/019f1234-5678-7abc-8def-0123456789ab/dashboard",
      );
    });
  });

  it("REQ-FE-002: refuses to navigate when the server omits the Space UID", async () => {
    (spaceApi.create as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "legacy-space-id",
      name: "my-space",
    });

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "+ Space" }));
    fireEvent.input(screen.getByLabelText("Space name"), {
      target: { value: "My space" },
    });
    fireEvent.input(screen.getByLabelText("Space slug"), {
      target: { value: "my-space" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create space" }));

    await waitFor(() => {
      expect(navigateMock).not.toHaveBeenCalled();
      expect(screen.getByRole("alert")).toBeInTheDocument();
    });
  });

  it("REQ-FE-002: labels the create-space field as a Space slug and explains its mutable metadata semantics", async () => {
    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "+ Space" }));

    expect(screen.getByLabelText("Space name")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("e.g. Project notes"))
      .toBeInTheDocument();
    expect(screen.getByLabelText("Space slug")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("e.g. team-notes")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Use letters, numbers, hyphens, or underscores. This human-readable metadata can be changed later; remote operations use the server-returned Space UID.",
      ),
    ).toBeInTheDocument();
  });

  it("REQ-FE-002: rewrites invalid Space slug backend errors into user-facing guidance", async () => {
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

    fireEvent.click(screen.getByRole("button", { name: "+ Space" }));
    fireEvent.input(screen.getByLabelText("Space name"), {
      target: { value: "My space" },
    });
    fireEvent.input(screen.getByLabelText("Space slug"), {
      target: { value: "My Space" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create space" }));

    await waitFor(() => {
      expect(
        screen.getByText(
          "Space slugs can use only letters, numbers, hyphens, and underscores.",
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
      .mockResolvedValueOnce({
        id: "legacy-space-id",
        space_uid: "019f1234-5678-7abc-8def-0123456789ab",
        name: "my-space",
      });
    (authApi.loginWithPasskey as ReturnType<typeof vi.fn>).mockResolvedValue(
      undefined,
    );

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("No spaces available.")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "+ Space" }));
    fireEvent.input(screen.getByLabelText("Space name"), {
      target: { value: "My space" },
    });
    fireEvent.input(screen.getByLabelText("Space slug"), {
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
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/019f1234-5678-7abc-8def-0123456789ab/dashboard",
      );
    });
  });

  it("REQ-FE-056: does not show persistent auth guidance during normal space listing", async () => {
    (spaceApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([
      { id: "default", name: "default" },
    ]);

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByText("Available Spaces")).toBeInTheDocument();
      expect(screen.getByRole("link", { name: "default" }))
        .toBeInTheDocument();
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
    expect(within(spacesList).getAllByRole("listitem")).toHaveLength(2);
    expect(within(spacesList).getByRole("link", { name: "default" }))
      .toHaveAttribute(
        "href",
        "/spaces/default/dashboard",
      );
  });

  it("REQ-UX-LIST-001: opens spaces through full-row links with an unboxed secondary settings action", async () => {
    (spaceApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        id: "default",
        name: "Default",
        slug: "default",
        created_at: "2025-01-01T00:00:00Z",
      },
    ]);

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByRole("list", { name: "Spaces" }))
        .toBeInTheDocument();
    });

    const spacesList = screen.getByRole("list", { name: "Spaces" });
    // No table semantics and no Open column duplicating row activation.
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    expect(screen.queryByRole("columnheader")).not.toBeInTheDocument();
    expect(within(spacesList).queryByText("Open")).not.toBeInTheDocument();
    // No boxed chevron and no in-row cards.
    expect(spacesList.querySelector(".spacesOpen")).toBeNull();
    expect(spacesList.querySelector(".ui-card")).toBeNull();
    const chevron = spacesList.querySelector(".rowListChevron")!;
    expect(chevron.tagName).toBe("SPAN");
    expect(chevron).toHaveAttribute("aria-hidden", "true");
    // Full-row link carries the space name; the gear is an unboxed sibling.
    const open = within(spacesList).getByRole("link", { name: "Default" });
    expect(open).toHaveAttribute("href", "/spaces/default/dashboard");
    const settings = within(spacesList).getByRole("link", {
      name: "Settings",
    });
    expect(settings).toHaveAttribute("href", "/spaces/default/settings");
    expect(open.contains(settings)).toBe(false);
  });

  it("REQ-UX-LIST-001: renders spaces as rows with icon-only secondary actions", async () => {
    (spaceApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        id: "default",
        name: "Default",
        slug: "default",
        created_at: "2025-01-01T00:00:00Z",
      },
    ]);

    render(() => <SpacesIndexRoute />);

    await waitFor(() => {
      expect(screen.getByRole("list", { name: "Spaces" }))
        .toBeInTheDocument();
    });

    const spacesList = screen.getByRole("list", { name: "Spaces" });
    // Single name only: no slug second line.
    expect(within(spacesList).getByText("Default")).toBeInTheDocument();
    expect(spacesList.querySelector("small")).toBeNull();
    expect(within(spacesList).queryByText("default", { selector: "small" }))
      .not.toBeInTheDocument();
    // No placeholder content: only real data and actions remain.
    expect(within(spacesList).queryByText("—")).toBeNull();
    // Icon-only secondary action keeps its accessible name.
    expect(
      within(spacesList).getByRole("link", { name: "Settings" }),
    ).toHaveAttribute("href", "/spaces/default/settings");
    const open = within(spacesList).getByRole("link", {
      name: "Default",
    });
    expect(open).toHaveAttribute("href", "/spaces/default/dashboard");
    expect(open.textContent).toContain("›");
    // The selector page has no Home back link.
    expect(screen.queryByRole("link", { name: "Back to Home" })).not
      .toBeInTheDocument();
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
