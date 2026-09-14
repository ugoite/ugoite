import "@testing-library/jest-dom/vitest";
import { createMemoryHistory, MemoryRouter, Route } from "@solidjs/router";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { GlobalShell } from "./GlobalShell";

describe("GlobalShell router navigation", () => {
  it("exposes only Spaces and About in global navigation", () => {
    const history = createMemoryHistory();
    history.set({ value: "/spaces", replace: true, scroll: false });

    render(() => (
      <MemoryRouter history={history}>
        <Route
          path="/spaces"
          component={() => (
            <GlobalShell title="Spaces" active="spaces">
              <p>Content</p>
            </GlobalShell>
          )}
        />
      </MemoryRouter>
    ));

    expect(screen.getAllByRole("link", { name: "Spaces" })).toHaveLength(2);
    expect(screen.getAllByRole("link", { name: "About" })).toHaveLength(2);
    expect(screen.queryByRole("link", { name: "Home" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Forms" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Search" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Settings" })).not
      .toBeInTheDocument();

    const currentLinks = screen.getAllByRole("link").filter((link) =>
      link.getAttribute("aria-current") === "page"
    );
    expect(currentLinks).toHaveLength(2);
    expect(
      currentLinks.every((link) => link.getAttribute("href") === "/spaces"),
    ).toBe(true);
    for (const link of screen.getAllByRole("link", { name: "Spaces" })) {
      expect(link).toHaveClass("active");
    }
  });

  it("uses the exact About match for the About footer link", () => {
    const history = createMemoryHistory();
    history.set({ value: "/about", replace: true, scroll: false });

    render(() => (
      <MemoryRouter history={history}>
        <Route
          path="/about"
          component={() => (
            <GlobalShell title="About" active="about">
              <p>Content</p>
            </GlobalShell>
          )}
        />
      </MemoryRouter>
    ));

    const currentLinks = screen.getAllByRole("link").filter((link) =>
      link.getAttribute("aria-current") === "page"
    );
    expect(currentLinks).toHaveLength(2);
    expect(
      currentLinks.every((link) => link.getAttribute("href") === "/about"),
    ).toBe(true);
    for (const link of screen.getAllByRole("link", { name: "About" })) {
      expect(link).toHaveClass("active");
    }
  });
});
