import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
    window.history.replaceState(null, "", "/");
  });

  afterEach(() => {
    vi.restoreAllMocks();
    window.history.replaceState(null, "", "/");
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

  it("logs one privacy-safe local diagnostic after the fallback persists", async () => {
    const error = new TypeError(
      "Private Form title 4546c198-284b-4f53-9a11-c51edc899004 access_token=secret",
    );
    error.stack = [
      error.message,
      "    at load (https://tenant.example/spaces/4546c198-284b-4f53-9a11-c51edc899004/forms/private-entry-name.js?access_token=secret:48:12)",
    ].join("\n");
    const report = vi.spyOn(console, "error").mockImplementation(() => undefined);
    window.history.replaceState(
      null,
      "",
      "/spaces/4546c198-284b-4f53-9a11-c51edc899004/forms/Private%20Form%20title/entries?access_token=secret",
    );

    function BrokenPrivatePage() {
      throw error;
    }

    render(() => (
      <AppErrorBoundary>
        <BrokenPrivatePage />
      </AppErrorBoundary>
    ));

    await waitFor(() => expect(report).toHaveBeenCalledTimes(1));

    expect(report).toHaveBeenCalledWith(
      "[ugoite:app-error]",
      expect.objectContaining({
        category: "app-error-boundary",
        errorType: "TypeError",
        routeFamily: "/spaces/:space/forms",
        stackLocations: ["javascript:48:12"],
      }),
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "This page could not be displayed",
    );
    expect(JSON.stringify(report.mock.calls)).not.toContain(
      "Private Form title",
    );
    expect(JSON.stringify(report.mock.calls)).not.toContain(
      "4546c198-284b-4f53-9a11-c51edc899004",
    );
    expect(JSON.stringify(report.mock.calls)).not.toContain("access_token");
    expect(JSON.stringify(report.mock.calls)).not.toContain("tenant.example");
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
