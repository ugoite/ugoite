import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { SpaceShell } from "./SpaceShell";

const spaceStoreState = vi.hoisted(() => ({
  initialSpaces: [] as Array<{
    space_uid: string;
    name: string;
    created_at: string;
  }>,
  loadSpaces: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  A: (props: Record<string, unknown>) => {
    const { children, ...rest } = props;
    return <a {...(rest as never)}>{children as never}</a>;
  },
  useNavigate: () => vi.fn(),
  useParams: () => ({ space_id: "my-space-uid" }),
}));

vi.mock("~/lib/space-store", () => ({
  createSpaceStore: () => {
    const [spaces, setSpaces] = createSignal(spaceStoreState.initialSpaces);
    return {
      spaces,
      loadSpaces: async () => {
        const loaded = await spaceStoreState.loadSpaces();
        if (Array.isArray(loaded)) setSpaces(loaded);
        return "my-space-uid";
      },
      selectSpace: vi.fn(),
    };
  },
}));

describe("v5 SpaceShell", () => {
  beforeEach(() => {
    setLocale("en");
    spaceStoreState.initialSpaces = [
      { space_uid: "my-space-uid", name: "My Space", created_at: "" },
      { space_uid: "other-space-uid", name: "Other Space", created_at: "" },
    ];
    spaceStoreState.loadSpaces.mockResolvedValue(
      spaceStoreState.initialSpaces,
    );
  });
  it("reaches the Forms workspace from desktop and mobile primary navigation", () => {
    const { container } = render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));
    expect(screen.getByText("Content")).toBeInTheDocument();
    const expected: Array<[string, string]> = [
      ["Home", "/spaces/my-space-uid/dashboard"],
      ["Assets", "/spaces/my-space-uid/assets"],
      ["Forms", "/spaces/my-space-uid/forms"],
      ["Search", "/spaces/my-space-uid/search"],
      ["History", "/spaces/my-space-uid/history"],
      ["Settings", "/spaces/my-space-uid/settings"],
    ];
    for (const [name, href] of expected) {
      const links = screen.getAllByRole("link", { name });
      expect(links.length).toBeGreaterThanOrEqual(2);
      for (const link of links) {
        expect(link).toHaveAttribute("href", href);
      }
    }
    // Forms is a primary destination: desktop sidebar plus the mobile
    // bottom navigation each link to the Forms workspace.
    const bottomNav = container.querySelector(".bottomNav");
    expect(bottomNav).toBeInTheDocument();
    const primaryLabels = [...bottomNav!.querySelectorAll("a")].map((link) =>
      link.textContent
    );
    expect(primaryLabels).toEqual(
      expect.arrayContaining(["Home", "Forms", "Search", "History"]),
    );
    // Desktop groups Knowledge/Explore/Recovery; mobile More holds the rest.
    expect(screen.getByText("Knowledge")).toBeInTheDocument();
    expect(screen.getByText("Explore")).toBeInTheDocument();
    expect(screen.getByText("Recovery")).toBeInTheDocument();
    expect(screen.getByText("More")).toBeInTheDocument();
  });
  it("marks the selected destination in desktop and mobile navigation", () => {
    render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="forms">
        <p>Content</p>
      </SpaceShell>
    ));
    for (const link of screen.getAllByRole("link", { name: "Forms" })) {
      expect(link).toHaveClass("active");
    }
  });
  it("encodes Space segments across desktop and mobile targets", () => {
    render(() => (
      <SpaceShell spaceId="space/with space" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));
    for (
      const link of screen.getAllByRole("link", { name: "Forms" })
    ) {
      expect(link).toHaveAttribute(
        "href",
        "/spaces/space%2Fwith%20space/forms",
      );
    }
    for (const link of screen.getAllByRole("link", { name: "History" })) {
      expect(link).toHaveAttribute(
        "href",
        "/spaces/space%2Fwith%20space/history",
      );
    }
  });
  it("opens account settings inside the current Space settings navigation", () => {
    render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect(screen.getByRole("menuitem", { name: "Account settings" }))
      .toHaveAttribute(
        "href",
        "/spaces/my-space-uid/settings?section=credentials",
      );
  });
  it("REQ-FE-058: keeps Space identity available while Spaces load", async () => {
    let resolveSpaces!: (
      spaces: typeof spaceStoreState.initialSpaces,
    ) => void;
    spaceStoreState.initialSpaces = [];
    spaceStoreState.loadSpaces.mockImplementation(
      () => new Promise((resolve) => resolveSpaces = resolve),
    );

    const { container } = render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));

    expect(container.querySelector(".loadingBar")).toBeNull();
    expect(container.querySelector(".ui-loading-bar")).toBeNull();
    expect(screen.getByRole("option", { name: "my-space-uid" })).toHaveValue(
      "my-space-uid",
    );
    resolveSpaces([
      { space_uid: "my-space-uid", name: "My Space", created_at: "" },
      { space_uid: "other-space-uid", name: "Other Space", created_at: "" },
    ]);
    expect(await screen.findByRole("option", { name: "My Space" }))
      .toHaveValue("my-space-uid");
    expect(screen.getByRole("option", { name: "My Space" })).toHaveValue(
      "my-space-uid",
    );
    expect(screen.getByRole("option", { name: "Other Space" })).toHaveValue(
      "other-space-uid",
    );
    expect(screen.getByRole("combobox", { name: "Space" })).toHaveValue(
      "my-space-uid",
    );
  });
  it("preserves Konase state while the utility panel is closed", () => {
    render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));

    const utility = screen.getByRole("button", { name: "Konase" });
    fireEvent.click(utility);
    const apiKey = screen.getByLabelText("Model API key");
    fireEvent.input(apiKey, { target: { value: "test-key" } });
    fireEvent.click(utility);

    expect(screen.getByLabelText("Model API key")).toHaveValue("test-key");
    expect(screen.getByLabelText("Model API key").closest(".konasePopover"))
      .toHaveAttribute("hidden");

    fireEvent.click(utility);
    expect(screen.getByLabelText("Model API key")).toHaveValue("test-key");
  });
  it("keeps the topbar assistant icon-only with no visible Konase text", () => {
    const { container } = render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));

    const topbar = container.querySelector(".topbar");
    expect(topbar).toBeInTheDocument();
    const assistant = topbar!.querySelector("button.pill.iconpill");
    expect(assistant).toBeInTheDocument();
    expect(assistant).toHaveAttribute("aria-label", "Konase");
    expect(assistant!.querySelector(".assistantDot")).toBeInTheDocument();
    const visibleKonase = [...topbar!.querySelectorAll("span")].filter(
      (el) =>
        !el.classList.contains("ui-sr-only") &&
        el.textContent?.includes("Konase"),
    );
    expect(visibleKonase).toHaveLength(0);
    // Account menu stays visible next to the icon-only assistant.
    expect(
      screen.getByRole("button", { name: "Account" }),
    ).toBeInTheDocument();
  });
  it("does not render a decorative topbar overflow control", () => {
    const { container } = render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));

    expect(container.querySelector(".topbarMore")).toBeNull();
  });
  it("localizes navigation", () => {
    setLocale("ja");
    render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));
    expect(screen.getAllByRole("link", { name: "ホーム" }).length)
      .toBeGreaterThan(0);
  });
  it("keeps shell and children mounted without a global loading bar", () => {
    const { container } = render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Child content</p>
      </SpaceShell>
    ));
    expect(screen.getByText("Child content")).toBeInTheDocument();
    expect(screen.getAllByRole("link", { name: "Home" })[0])
      .toBeInTheDocument();
    expect(container.querySelector(".loadingBar")).toBeNull();
    expect(container.querySelector(".ui-loading-bar")).toBeNull();
  });
  it("keeps the route space selected when the route changes", () => {
    const [spaceId, setSpaceId] = createSignal("my-space-uid");
    render(() => (
      <SpaceShell spaceId={spaceId()} activeNavigation="home">
        <p>Space content</p>
      </SpaceShell>
    ));
    setSpaceId("other-space-uid");
    expect(screen.getByRole("combobox", { name: "Space" })).toHaveValue(
      "other-space-uid",
    );
  });
});
