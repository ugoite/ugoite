import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { SpaceShell } from "./SpaceShell";

vi.mock("@solidjs/router", () => ({
  A: (props: Record<string, unknown>) => {
    const { children, ...rest } = props;
    return <a {...(rest as never)}>{children as never}</a>;
  },
  useNavigate: () => vi.fn(),
  useParams: () => ({ space_id: "my-space-uid" }),
}));

vi.mock("~/lib/space-store", () => ({
  createSpaceStore: () => ({
    spaces: () => [
      {
        id: "legacy-my-space",
        space_uid: "my-space-uid",
        name: "My Space",
        created_at: "",
      },
      {
        id: "legacy-other-space",
        space_uid: "other-space-uid",
        name: "Other Space",
        created_at: "",
      },
    ],
    loadSpaces: vi.fn().mockResolvedValue("my-space-uid"),
    selectSpace: vi.fn(),
  }),
}));

describe("v5 SpaceShell", () => {
  beforeEach(() => {
    setLocale("en");
  });
  it("renders the four persistent navigation destinations and children", () => {
    render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));
    expect(screen.getByText("Content")).toBeInTheDocument();
    expect(screen.getAllByRole("link", { name: "Home" })[0]).toHaveAttribute(
      "href",
      "/spaces/my-space-uid/dashboard",
    );
    expect(screen.getAllByRole("link", { name: "Forms" })[0]).toHaveAttribute(
      "href",
      "/spaces/my-space-uid/forms",
    );
    expect(screen.getAllByRole("link", { name: "Search" })[0]).toHaveAttribute(
      "href",
      "/spaces/my-space-uid/search",
    );
    expect(screen.getAllByRole("link", { name: "Settings" })[0])
      .toHaveAttribute("href", "/spaces/my-space-uid/settings");
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
  it("offers the other available spaces in the workspace selector", () => {
    render(() => (
      <SpaceShell spaceId="my-space-uid" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));
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
