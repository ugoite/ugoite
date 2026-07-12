// REQ-FE-010: SpaceShell layout component
import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import { SpaceShell } from "./SpaceShell";
import { setLocale } from "~/lib/i18n";
import { loadingState } from "~/lib/loading";

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    classList?: Record<string, boolean>;
    children: unknown;
  }) => {
    const classes = [
      props.class,
      ...(props.classList
        ? Object.keys(props.classList).filter((k) => props.classList?.[k])
        : []),
    ]
      .filter(Boolean)
      .join(" ");
    return (
      <a href={props.href} class={classes}>
        {props.children}
      </a>
    );
  },
}));

describe("SpaceShell", () => {
  beforeEach(() => {
    setLocale("en");
  });

  it("renders children", () => {
    render(() => (
      <SpaceShell spaceId="my-space">
        <div>Child content</div>
      </SpaceShell>
    ));
    expect(screen.getByText("Child content")).toBeInTheDocument();
  });

  it("renders the persistent workspace navigation", () => {
    render(() => (
      <SpaceShell spaceId="my-space">
        <div>Content</div>
      </SpaceShell>
    ));
    const homeLink = screen.getByRole("link", { name: "Home" });
    expect(homeLink).toHaveAttribute("href", "/spaces/my-space/dashboard");
    expect(screen.getByRole("link", { name: "Forms" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Search" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Settings" })).toBeInTheDocument();
  });

  it("applies active class to the current workspace section", () => {
    render(() => (
      <SpaceShell spaceId="my-space" activeTopTab="dashboard">
        <div>Content</div>
      </SpaceShell>
    ));
    const homeLink = screen.getByRole("link", { name: "Home" });
    expect(homeLink).toHaveClass("ui-global-nav-item-active");
  });

  it("does not render legacy Entries navigation", () => {
    render(() => (
      <SpaceShell spaceId="my-space" showBottomTabs={false}>
        <div>Content</div>
      </SpaceShell>
    ));
    expect(screen.queryByRole("link", { name: "Entries" })).not
      .toBeInTheDocument();
  });

  it("REQ-FE-040: makes Forms the active form workspace", () => {
    render(() => (
      <SpaceShell
        spaceId="my-space"
        showBottomTabs={true}
        activeBottomTab="grid"
      >
        <div>Content</div>
      </SpaceShell>
    ));
    const formsLink = screen.getByRole("link", { name: "Forms" });
    expect(formsLink).toHaveClass("ui-global-nav-item-active");
  });

  it("shows loading bar when loading", () => {
    loadingState.start();
    render(() => (
      <SpaceShell spaceId="my-space">
        <div>Content</div>
      </SpaceShell>
    ));
    expect(document.querySelector(".ui-loading-bar")).toBeInTheDocument();
    loadingState.stop();
  });
});
