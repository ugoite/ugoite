import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "~/lib/i18n";
import {
  recoverFromVitePreloadError,
  type VitePreloadErrorEvent,
} from "~/lib/vite-preload-recovery";
import { AppErrorBoundary } from "./AppErrorBoundary";

function BrokenPage() {
  throw new Error("sensitive internal error");
}

describe("AppErrorBoundary", () => {
  beforeEach(() => {
    setLocale("en");
    window.sessionStorage.clear();
  });

  it("replaces uncaught route errors with a recoverable page", () => {
    render(() => (
      <AppErrorBoundary>
        <BrokenPage />
      </AppErrorBoundary>
    ));

    expect(screen.getByRole("alert")).toHaveTextContent(
      "This page could not be displayed",
    );
    expect(screen.queryByText("sensitive internal error")).not
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Spaces" }))
      .toHaveAttribute("href", "/spaces");
    expect(screen.getByRole("button", { name: "Try again" }))
      .toBeInTheDocument();
  });

  it("resets the dynamic-import retry guard when the route changes", async () => {
    const [pathname, setPathname] = createSignal("/spaces/one/forms");
    const reload = () => undefined;
    const event = (): VitePreloadErrorEvent => {
      const value = new Event("vite:preloadError", { cancelable: true });
      Object.assign(value, {
        payload: new Error("Importing a module script failed."),
      });
      return value as VitePreloadErrorEvent;
    };

    render(() => (
      <AppErrorBoundary pathname={pathname()}>
        <p>Forms route</p>
      </AppErrorBoundary>
    ));

    expect(
      recoverFromVitePreloadError(
        event(),
        window.sessionStorage,
        reload,
        pathname(),
        1_000,
      ),
    ).toBe(true);
    expect(
      recoverFromVitePreloadError(
        event(),
        window.sessionStorage,
        reload,
        pathname(),
        1_001,
      ),
    ).toBe(false);

    setPathname("/spaces/one/entries/entry");

    await waitFor(() =>
      expect(
        recoverFromVitePreloadError(
          event(),
          window.sessionStorage,
          reload,
          "/spaces/one/forms",
          1_002,
        ),
      ).toBe(true)
    );
  });

  it("resets a captured route error when navigation changes the route", async () => {
    const [pathname, setPathname] = createSignal("/route-error");
    const [showDestination, setShowDestination] = createSignal(false);

    render(() => (
      <AppErrorBoundary pathname={pathname()}>
        {showDestination() ? <p>Destination route</p> : <BrokenPage />}
      </AppErrorBoundary>
    ));

    expect(screen.getByRole("alert")).toHaveTextContent(
      "This page could not be displayed",
    );

    setShowDestination(true);
    setPathname("/destination");

    await waitFor(() =>
      expect(screen.getByText("Destination route")).toBeInTheDocument()
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
