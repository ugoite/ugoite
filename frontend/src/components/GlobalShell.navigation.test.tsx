import "@testing-library/jest-dom/vitest";
import { createMemoryHistory, MemoryRouter, Route } from "@solidjs/router";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { GlobalShell } from "./GlobalShell";

describe("GlobalShell router navigation", () => {
  it("does not mark duplicate global links as current", () => {
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

    const currentLinks = screen.getAllByRole("link").filter((link) =>
      link.getAttribute("aria-current") === "page"
    );
    expect(currentLinks).toHaveLength(1);
    expect(currentLinks[0]).toHaveAttribute("href", "/spaces");
  });
});
