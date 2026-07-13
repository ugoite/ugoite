import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "~/lib/i18n";
import { loadingState } from "~/lib/loading";
import { SpaceShell } from "./SpaceShell";

describe("v5 SpaceShell", () => {
  beforeEach(() => setLocale("en"));
  it("renders the four persistent navigation destinations and children", () => {
    render(() => <SpaceShell spaceId="my-space" activeNavigation="home"><p>Content</p></SpaceShell>);
    expect(screen.getByText("Content")).toBeInTheDocument();
    expect(screen.getAllByRole("link", { name: "Home" })[0]).toHaveAttribute("href", "/spaces/my-space/dashboard");
    expect(screen.getAllByRole("link", { name: "Forms" })[0]).toHaveAttribute("href", "/spaces/my-space/forms");
    expect(screen.getAllByRole("link", { name: "Search" })[0]).toHaveAttribute("href", "/spaces/my-space/search");
    expect(screen.getAllByRole("link", { name: "Settings" })[0]).toHaveAttribute("href", "/spaces/my-space/settings");
  });
  it("marks the selected destination in desktop and mobile navigation", () => {
    render(() => <SpaceShell spaceId="my-space" activeNavigation="forms"><p>Content</p></SpaceShell>);
    for (const link of screen.getAllByRole("link", { name: "Forms" })) expect(link).toHaveClass("active");
  });
  it("localizes navigation", () => {
    setLocale("ja");
    render(() => <SpaceShell spaceId="my-space"><p>Content</p></SpaceShell>);
    expect(screen.getAllByRole("link", { name: "ホーム" }).length).toBeGreaterThan(0);
  });
  it("shows the v5 loading indicator", () => {
    loadingState.start();
    const { container } = render(() => <SpaceShell spaceId="my-space"><p>Content</p></SpaceShell>);
    expect(container.querySelector(".loadingBar")).toBeInTheDocument();
    loadingState.stop();
  });
});
