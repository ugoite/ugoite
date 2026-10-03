import { strict as assert } from "node:assert";
import type { Page } from "@playwright/test";
import {
  createSetupReadinessDiagnostics,
  formatSetupReadinessDiagnostics,
  observeSetupReadiness,
  safeUrlPath,
} from "./readiness-diagnostics.ts";

Deno.test("setup readiness diagnostics report safe navigation and asset state", () => {
  const listeners = new Map<string, Array<(value: never) => void>>();
  const mainFrame = {};
  const page = {
    mainFrame: () => mainFrame,
    on(event: string, listener: (value: never) => void) {
      const handlers = listeners.get(event) ?? [];
      handlers.push(listener);
      listeners.set(event, handlers);
      return this;
    },
  } as unknown as Page;
  const diagnostics = createSetupReadinessDiagnostics();
  const emit = (event: string, value: unknown) => {
    for (const listener of listeners.get(event) ?? []) listener(value as never);
  };

  observeSetupReadiness(page, diagnostics);
  const mainRequest = {
    isNavigationRequest: () => true,
    frame: () => mainFrame,
  };
  emit("response", {
    request: () => mainRequest,
    url: () => "https://localhost/setup?token=query-secret#secret=fragment-secret",
    status: () => 200,
    ok: () => true,
  });
  emit("requestfailed", {
    isNavigationRequest: () => false,
    frame: () => ({}),
    url: () =>
      "https://localhost/_build/assets/path-recovery-secret.js?token=asset-secret",
    failure: () => ({ errorText: "net::ERR_SETUP_SECRET" }),
  });
  emit("pageerror", {
    name: "recovery-secret",
    message: "page-error-secret",
  });
  emit("console", {
    type: () => "error",
    location: () => ({
      url: "https://localhost/settings/recovery-path-secret?token=console-secret",
    }),
    text: () => "console-secret",
  });

  const output = formatSetupReadinessDiagnostics(
    "https://localhost/setup?token=query-secret#secret=fragment-secret",
    diagnostics,
    { readyState: "complete", bodyElementCount: 0, bodyTextLength: 34 },
  );
  assert.equal(output.includes("path=/setup"), true);
  assert.equal(output.includes("documentResponses=attempt-1:/setup:http-200"), true);
  assert.equal(
    output.includes("assetFailures=attempt-1:/_build/<asset>.js:request-failed"),
    true,
  );
  assert.equal(output.includes("pageErrors=attempt-1:Error"), true);
  assert.equal(
    output.includes("consoleErrors=attempt-1:<path>"),
    true,
  );
  assert.equal(output.includes("bodyElementCount=0"), true);
  assert.equal(output.includes("bodyTextLength=34"), true);
  for (
    const secret of [
      "query-secret",
      "fragment-secret",
      "asset-secret",
      "path-recovery-secret",
      "recovery-secret",
      "page-error-secret",
      "console-secret",
      "recovery-path-secret",
      "ERR_SETUP_SECRET",
    ]
  ) {
    assert.equal(output.includes(secret), false, `diagnostics leaked ${secret}`);
  }
  assert.equal(safeUrlPath("not a url"), "<unavailable>");
  assert.equal(safeUrlPath("https://localhost/setup/path-secret"), "<path>");
  assert.equal(
    safeUrlPath("https://localhost/_build/assets/path-secret.js"),
    "/_build/<asset>.js",
  );
});
