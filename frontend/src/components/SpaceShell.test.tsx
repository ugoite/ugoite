import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { loadingState } from "~/lib/loading";
import { SpaceShell } from "./SpaceShell";

vi.mock("~/lib/space-store", () => ({
  createSpaceStore: () => ({
    spaces: () => [
      { id: "my-space", name: "My Space", created_at: "" },
      { id: "other-space", name: "Other Space", created_at: "" },
    ],
    loadSpaces: vi.fn().mockResolvedValue("my-space"),
    selectSpace: vi.fn(),
  }),
}));

describe("v5 SpaceShell", () => {
  beforeEach(() => {
    setLocale("en");
  });
  it("renders the four persistent navigation destinations and children", () => {
    render(() => (
      <SpaceShell spaceId="my-space" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));
    expect(screen.getByText("Content")).toBeInTheDocument();
    expect(screen.getAllByRole("link", { name: "Home" })[0]).toHaveAttribute(
      "href",
      "/spaces/my-space/dashboard",
    );
    expect(screen.getAllByRole("link", { name: "Forms" })[0]).toHaveAttribute(
      "href",
      "/spaces/my-space/forms",
    );
    expect(screen.getAllByRole("link", { name: "Search" })[0]).toHaveAttribute(
      "href",
      "/spaces/my-space/search",
    );
    expect(screen.getAllByRole("link", { name: "Settings" })[0])
      .toHaveAttribute("href", "/spaces/my-space/settings");
  });
  it("marks the selected destination in desktop and mobile navigation", () => {
    render(() => (
      <SpaceShell spaceId="my-space" activeNavigation="forms">
        <p>Content</p>
      </SpaceShell>
    ));
    for (const link of screen.getAllByRole("link", { name: "Forms" })) {
      expect(link).toHaveClass("active");
    }
  });
  it("opens account settings inside the current Space settings navigation", () => {
    render(() => (
      <SpaceShell spaceId="my-space" activeNavigation="home">
        <p>Content</p>
      </SpaceShell>
    ));

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect(screen.getByRole("menuitem", { name: "Account settings" }))
      .toHaveAttribute(
        "href",
        "/spaces/my-space/settings?section=credentials",
      );
  });
  it("offers the other available spaces in the workspace selector", () => {
    render(() => (
      <SpaceShell spaceId="my-space">
        <p>Content</p>
      </SpaceShell>
    ));
    expect(screen.getByRole("option", { name: "My Space" })).toHaveValue(
      "my-space",
    );
    expect(screen.getByRole("option", { name: "Other Space" })).toHaveValue(
      "other-space",
    );
    expect(screen.getByRole("combobox", { name: "Space" })).toHaveValue(
      "my-space",
    );
  });
  it("localizes navigation", () => {
    setLocale("ja");
    render(() => (
      <SpaceShell spaceId="my-space">
        <p>Content</p>
      </SpaceShell>
    ));
    expect(screen.getAllByRole("link", { name: "ホーム" }).length)
      .toBeGreaterThan(0);
  });
  it("shows the v5 loading indicator", () => {
    loadingState.start();
    const { container } = render(() => (
      <SpaceShell spaceId="my-space">
        <p>Content</p>
      </SpaceShell>
    ));
    expect(container.querySelector(".loadingBar")).toBeInTheDocument();
    loadingState.stop();
  });
  it("keeps the route space selected when the route changes", () => {
    const [spaceId, setSpaceId] = createSignal("my-space");
    render(() => (
      <SpaceShell spaceId={spaceId()}><p>Space content</p></SpaceShell>
    ));
    setSpaceId("other-space");
    expect(screen.getByRole("combobox", { name: "Space" })).toHaveValue(
      "other-space",
    );
  });
});
