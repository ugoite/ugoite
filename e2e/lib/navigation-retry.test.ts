// PR-01: focused unit tests for the E2E navigation retry policy.
//
// The retry policy allows exactly ONE recovery and ONLY for browser-level
// environment failures (document load failure, recognized
// ERR_NETWORK_CHANGED, connection reset/refused, static JS chunk request
// failure, 2xx document with an empty DOM plus a frontend asset environment
// error). Product verdicts (HTTP 4xx/5xx, API validation/authorization,
// WebAuthn ceremony failure, visible application error, assertion failure,
// product state mismatch) must NEVER retry.
import { strict as assert } from "node:assert";
import type { Browser, BrowserContext, Page } from "@playwright/test";
import {
  classifyNavigationFailure,
  isEnvironmentFailure,
  isProductFailure,
  safeNavigationFailure,
  safeNetworkFailureCode,
  shouldRetryEnvironmentFailure,
} from "./security-context.ts";
import {
  createSetupReadinessDiagnostics,
  formatSetupReadinessDiagnostics,
  observeSetupReadiness,
} from "./readiness-diagnostics.ts";
import {
  gotoPageWithOneEnvironmentRetry,
  gotoWithOneEnvironmentRetry,
} from "./navigation-retry.ts";

const NETWORK_CHANGED = new Error(
  "page.goto: net::ERR_NETWORK_CHANGED at https://localhost/setup",
);

Deno.test("safe navigation failure summaries omit URLs and response bodies", () => {
  const navigation = safeNavigationFailure(
    new Error(
      "page.goto: net::ERR_NETWORK_CHANGED at https://localhost/setup#secret=setup-secret",
    ),
  );
  assert.equal(navigation, "ERR_NETWORK_CHANGED");
  assert.equal(navigation.includes("setup-secret"), false);

  const http = safeNavigationFailure(
    new Error(
      "navigation to https://localhost/setup#secret=setup-secret returned 503: recovery-code",
    ),
  );
  assert.equal(http, "http-503");
  assert.equal(http.includes("setup-secret"), false);
  assert.equal(http.includes("recovery-code"), false);

  const customName = new Error("recovery-secret");
  customName.name = "setup-secret";
  assert.equal(safeNavigationFailure(customName), "navigation-error");

  const customCode = new Error(
    "page.goto: net::ERR_SETUP_SECRET at https://localhost/setup#secret=recovery-secret",
  );
  assert.equal(safeNavigationFailure(customCode), "navigation-error");
  assert.equal(
    safeNetworkFailureCode("net::ERR_SETUP_SECRET"),
    "request-failed",
  );
  assert.equal(
    safeNetworkFailureCode("net::ERR_CONNECTION_RESET"),
    "ERR_CONNECTION_RESET",
  );
});

Deno.test("classifier: recognized environment errors earn one recovery", () => {
  for (
    const error of [
      NETWORK_CHANGED,
      new Error("net::ERR_INTERNET_DISCONNECTED"),
      new Error("page.goto: NS_ERROR_FAILURE"),
      new Error("connect ECONNREFUSED 127.0.0.1:8000"),
      new Error("read ECONNRESET"),
      new Error("getaddrinfo ENOTFOUND localhost"),
    ]
  ) {
    assert.equal(isEnvironmentFailure(error), true, String(error));
    assert.equal(isProductFailure(error), false, String(error));
    assert.equal(
      classifyNavigationFailure(error),
      "retry",
      String(error),
    );
    assert.equal(shouldRetryEnvironmentFailure(error), true, String(error));
  }
});

Deno.test("classifier: HTTP 4xx/5xx fixtures never retry", () => {
  for (
    const error of [
      new Error("navigation to http://localhost/setup returned 422: {}"),
      new Error("navigation to http://localhost/setup returned 500: {}"),
      new Error("setup finish returned 403: Forbidden"),
      new Error("Request failed with status 404"),
    ]
  ) {
    assert.equal(classifyNavigationFailure(error), "no-retry", String(error));
  }
  // A bare network error paired with a 4xx/5xx document status is still a
  // product verdict: the HTTP status takes precedence.
  assert.equal(
    classifyNavigationFailure(NETWORK_CHANGED, { status: 422 }),
    "no-retry",
  );
  assert.equal(
    classifyNavigationFailure(NETWORK_CHANGED, { status: 503 }),
    "no-retry",
  );
});

Deno.test("classifier: setup ceremony failures never retry", () => {
  for (
    const error of [
      new Error("NotAllowedError: passkey ceremony failed"),
      new Error("InvalidStateError: authenticator already registered"),
      new Error("WebAuthn ceremony failed: user did not consent"),
      new Error("second Passkey finish returned 422"),
      new Error("administrator passkey setup failed: InvalidStateError"),
    ]
  ) {
    assert.equal(isProductFailure(error), true, String(error));
    assert.equal(classifyNavigationFailure(error), "no-retry", String(error));
  }
});

Deno.test("classifier: application errors and assertion failures never retry", () => {
  for (
    const error of [
      new Error("[application] setup: heading did not appear"),
      new Error("expect(locator).toBeVisible failed"),
      new Error("expect(page).toHaveURL(/spaces) failed"),
      new Error("ORIGIN_MISMATCH: unsafe browser requests"),
      new Error("NODE_UNINITIALIZED: complete node setup"),
      new Error("VALIDATION failed for display_name"),
    ]
  ) {
    assert.equal(classifyNavigationFailure(error), "no-retry", String(error));
  }
});

Deno.test("classifier: static chunk failure and empty 2xx document retry", () => {
  const chunk = new Error(
    "Failed to fetch dynamically imported module /_build/entry.js",
  );
  assert.equal(
    classifyNavigationFailure(chunk, {
      assetError:
        "http://localhost/_build/entry.js :: net::ERR_NETWORK_CHANGED",
    }),
    "retry",
  );
  assert.equal(
    classifyNavigationFailure(new Error("setup form did not become visible"), {
      status: 200,
      bodySnippet: "",
      assetError:
        "http://localhost/_build/entry.js :: net::ERR_NETWORK_CHANGED",
    }),
    "retry",
  );
  // Same empty document WITHOUT an asset environment error is a product
  // state mismatch: no retry.
  assert.equal(
    classifyNavigationFailure(new Error("setup form did not become visible"), {
      status: 200,
      bodySnippet: "",
    }),
    "no-retry",
  );
  assert.equal(
    classifyNavigationFailure(new Error("readiness check failed"), {
      status: 200,
      bodySnippet: "",
      assetError: "http://localhost/_build/entry.js :: http-404",
    }),
    "no-retry",
  );
  assert.equal(
    classifyNavigationFailure(new Error("readiness check failed"), {
      status: 200,
      bodyText: "Server rendered setup form",
      assetError:
        "http://localhost/_build/entry.js :: net::ERR_NETWORK_CHANGED",
    }),
    "retry",
  );
  assert.equal(
    classifyNavigationFailure(new Error("VALIDATION failed for display_name"), {
      status: 200,
      bodyText: "Validation failed",
      assetError:
        "http://localhost/_build/entry.js :: net::ERR_NETWORK_CHANGED",
    }),
    "no-retry",
  );
});

// --- Helper-level harness with fake Playwright objects. ---

type FakePageEvents = {
  emit(event: string, value?: unknown): void;
  setBodyText(value: string): void;
};

type FakeGoto = (
  url: string,
  page: FakePageEvents,
) => Promise<
  { status(): number; ok(): boolean; text(): Promise<string> } | null
>;

function fakeBrowser(gotos: FakeGoto[]): {
  browser: Browser;
  contextsCreated: () => number;
  contextsClosed: () => number;
} {
  let contexts = 0;
  let closedContexts = 0;
  let gotoCalls = 0;
  const browser = {
    async newContext(): Promise<BrowserContext> {
      contexts++;
      const listeners: Record<string, Array<(...args: never[]) => void>> = {};
      const mainFrame = {};
      let bodyText = "";
      const fakePage: FakePageEvents = {
        emit(event, value) {
          for (const listener of listeners[event] ?? []) {
            listener(value as never);
          }
        },
        setBodyText(value) {
          bodyText = value;
        },
      };
      const page = {
        mainFrame: () => mainFrame,
        on(event: string, listener: (...args: never[]) => void) {
          (listeners[event] ??= []).push(listener);
        },
        async goto(url: string) {
          const behavior = gotos[Math.min(gotoCalls++, gotos.length - 1)];
          return await behavior(url, fakePage);
        },
        async evaluate(): Promise<string> {
          return bodyText;
        },
      } as unknown as Page;
      return {
        async newPage(): Promise<Page> {
          return page;
        },
        async close(): Promise<void> {
          closedContexts++;
        },
      } as unknown as BrowserContext;
    },
  } as unknown as Browser;
  return {
    browser,
    contextsCreated: () => contexts,
    contextsClosed: () => closedContexts,
  };
}

const okResponse = () => ({
  status: () => 200,
  ok: () => true,
  text: async () => "",
});

Deno.test("gotoWithOneEnvironmentRetry: environment error recovers exactly once", async () => {
  const { browser, contextsCreated } = fakeBrowser([
    async () => {
      throw NETWORK_CHANGED;
    },
    async () => okResponse(),
  ]);
  const result = await gotoWithOneEnvironmentRetry(
    browser,
    "http://localhost/setup",
  );
  assert.equal(result.retried, true);
  assert.equal(contextsCreated(), 2);
});

function failedFrontendAsset() {
  return {
    isNavigationRequest: () => false,
    frame: () => ({}),
    url: () => "http://localhost/_build/assets/setup.js",
    failure: () => ({ errorText: "net::ERR_NETWORK_CHANGED" }),
  };
}

function failedFrontendAssetResponse(status: number) {
  return {
    request: () => ({
      isNavigationRequest: () => false,
      frame: () => ({}),
    }),
    url: () => "http://localhost/_build/assets/setup.js",
    status: () => status,
    ok: () => status >= 200 && status < 300,
  };
}

Deno.test(
  "gotoWithOneEnvironmentRetry: transient setup asset failure retries and becomes ready",
  async () => {
    const diagnostics = createSetupReadinessDiagnostics();
    let readinessChecks = 0;
    const { browser, contextsCreated, contextsClosed } = fakeBrowser([
      async (_url, page) => {
        page.setBodyText("Server rendered setup form");
        page.emit("requestfailed", failedFrontendAsset());
        return okResponse();
      },
      async (_url, page) => {
        page.setBodyText("Display name");
        return okResponse();
      },
    ]);

    const result = await gotoWithOneEnvironmentRetry(
      browser,
      "http://localhost/setup#secret=hidden",
      {
        prepare: async (page) => observeSetupReadiness(page, diagnostics),
        waitForReady: async () => {
          readinessChecks++;
        },
      },
    );

    assert.equal(result.retried, true);
    assert.equal(contextsCreated(), 2);
    assert.equal(contextsClosed(), 1);
    assert.equal(readinessChecks, 1);
    const output = formatSetupReadinessDiagnostics(
      "http://localhost/setup#secret=hidden",
      diagnostics,
    );
    assert.equal(
      output.includes(
        "assetFailures=attempt-1:/_build/<asset>.js:ERR_NETWORK_CHANGED",
      ),
      true,
    );
    assert.equal(output.includes("navigationAttempts=2"), true);
    assert.equal(output.includes("hidden"), false);
  },
);

Deno.test(
  "gotoWithOneEnvironmentRetry: a failed retry navigation does not start a third attempt",
  async () => {
    const { browser, contextsCreated, contextsClosed } = fakeBrowser([
      async (_url, page) => {
        page.emit("requestfailed", failedFrontendAsset());
        return okResponse();
      },
      async () => {
        throw NETWORK_CHANGED;
      },
      async () => okResponse(),
    ]);

    await assert.rejects(
      () =>
        gotoWithOneEnvironmentRetry(browser, "http://localhost/setup", {
          waitForReady: async () => await new Promise(() => {}),
        }),
      /ERR_NETWORK_CHANGED/,
    );
    assert.equal(contextsCreated(), 2);
    assert.equal(contextsClosed(), 2);
  },
);

Deno.test(
  "gotoWithOneEnvironmentRetry: HTTP 200 script MIME failure fails promptly without retry",
  async () => {
    const diagnostics = createSetupReadinessDiagnostics();
    const { browser, contextsCreated, contextsClosed } = fakeBrowser([
      async (_url, page) => {
        page.emit("console", {
          type: () => "error",
          text: () =>
            "Failed to load module script: server responded with a MIME type of text/html",
          location: () => ({
            url: "http://localhost/_build/assets/setup.js?secret=hidden",
          }),
        });
        return okResponse();
      },
    ]);

    let finalError: unknown;
    try {
      await gotoWithOneEnvironmentRetry(browser, "http://localhost/setup", {
        prepare: async (page) => observeSetupReadiness(page, diagnostics),
        waitForReady: async () => await new Promise(() => {}),
      });
    } catch (error) {
      finalError = error;
    }

    assert.equal(safeNavigationFailure(finalError), "readiness-failed");
    assert.equal(contextsCreated(), 1);
    assert.equal(contextsClosed(), 1);
    assert.equal(
      formatSetupReadinessDiagnostics("http://localhost/setup", diagnostics)
        .includes("consoleErrors=attempt-1:/_build/<asset>.js"),
      true,
    );
    assert.equal(
      formatSetupReadinessDiagnostics("http://localhost/setup", diagnostics)
        .includes("secret"),
      false,
    );
  },
);

Deno.test(
  "gotoWithOneEnvironmentRetry: persistent asset network failure fails with both attempts",
  async () => {
    const diagnostics = createSetupReadinessDiagnostics();
    const { browser, contextsCreated, contextsClosed } = fakeBrowser([
      async (_url, page) => {
        page.emit("requestfailed", failedFrontendAsset());
        return okResponse();
      },
      async (_url, page) => {
        page.emit("requestfailed", failedFrontendAsset());
        return okResponse();
      },
    ]);

    let finalError: unknown;
    try {
      await gotoWithOneEnvironmentRetry(browser, "http://localhost/setup", {
        prepare: async (page) => observeSetupReadiness(page, diagnostics),
        waitForReady: async () => await new Promise(() => {}),
      });
    } catch (error) {
      finalError = error;
    }

    assert.equal(safeNavigationFailure(finalError), "readiness-failed");
    assert.equal(contextsCreated(), 2);
    assert.equal(contextsClosed(), 2);
    const finalDiagnostic = `setup navigation failed (${
      safeNavigationFailure(finalError)
    }); ${
      formatSetupReadinessDiagnostics(
        "http://localhost/setup",
        diagnostics,
      )
    }`;
    assert.equal(
      finalDiagnostic.includes(
        "attempt-1:/_build/<asset>.js:ERR_NETWORK_CHANGED",
      ),
      true,
    );
    assert.equal(
      finalDiagnostic.includes(
        "attempt-2:/_build/<asset>.js:ERR_NETWORK_CHANGED",
      ),
      true,
    );
    assert.equal(finalDiagnostic.includes("navigationAttempts=2"), true);
  },
);

Deno.test(
  "gotoWithOneEnvironmentRetry: missing frontend asset fails promptly without retry",
  async () => {
    const diagnostics = createSetupReadinessDiagnostics();
    const { browser, contextsCreated, contextsClosed } = fakeBrowser([
      async (_url, page) => {
        page.emit("response", failedFrontendAssetResponse(404));
        return okResponse();
      },
    ]);
    let finalError: unknown;
    try {
      await gotoWithOneEnvironmentRetry(browser, "http://localhost/setup", {
        prepare: async (page) => observeSetupReadiness(page, diagnostics),
        waitForReady: async () => await new Promise(() => {}),
      });
    } catch (error) {
      finalError = error;
    }

    assert.equal(safeNavigationFailure(finalError), "readiness-failed");
    assert.equal(contextsCreated(), 1);
    assert.equal(contextsClosed(), 1);
    const finalDiagnostic = formatSetupReadinessDiagnostics(
      "http://localhost/setup",
      diagnostics,
    );
    assert.equal(
      finalDiagnostic.includes("attempt-1:/_build/<asset>.js:http-404"),
      true,
    );
  },
);

Deno.test(
  "gotoWithOneEnvironmentRetry: frontend startup error fails promptly without retry",
  async () => {
    const diagnostics = createSetupReadinessDiagnostics();
    const { browser, contextsCreated, contextsClosed } = fakeBrowser([
      async (_url, page) => {
        page.emit("pageerror", new SyntaxError("invalid setup bundle"));
        return okResponse();
      },
    ]);
    let finalError: unknown;
    try {
      await gotoWithOneEnvironmentRetry(browser, "http://localhost/setup", {
        prepare: async (page) => observeSetupReadiness(page, diagnostics),
        waitForReady: async () => await new Promise(() => {}),
      });
    } catch (error) {
      finalError = error;
    }

    assert.equal(safeNavigationFailure(finalError), "readiness-failed");
    assert.equal(contextsCreated(), 1);
    assert.equal(contextsClosed(), 1);
    assert.equal(
      formatSetupReadinessDiagnostics("http://localhost/setup", diagnostics)
        .includes("pageErrors=attempt-1:SyntaxError"),
      true,
    );
  },
);

Deno.test("gotoWithOneEnvironmentRetry: 4xx/5xx fixture does not retry", async () => {
  const { browser, contextsCreated } = fakeBrowser([
    async () => ({
      status: () => 422,
      ok: () => false,
      text: async () => '{"code":"VALIDATION"}',
    }),
  ]);
  await assert.rejects(
    () =>
      gotoWithOneEnvironmentRetry(browser, "http://localhost/setup", {
        requireOkResponse: true,
      }),
    /returned 422/,
  );
  assert.equal(contextsCreated(), 1);
});

Deno.test("gotoWithOneEnvironmentRetry: ceremony failure does not retry", async () => {
  const { browser, contextsCreated } = fakeBrowser([async () => okResponse()]);
  await assert.rejects(
    () =>
      gotoWithOneEnvironmentRetry(browser, "http://localhost/setup", {
        waitForReady: async () => {
          throw new Error("NotAllowedError: passkey ceremony failed");
        },
      }),
    /NotAllowedError/,
  );
  assert.equal(contextsCreated(), 1);
});

Deno.test("gotoPageWithOneEnvironmentRetry: same-page retry policy", async () => {
  let calls = 0;
  const page = {
    on() {},
    async goto(): Promise<null> {
      calls++;
      if (calls === 1) throw NETWORK_CHANGED;
      return null;
    },
  } as unknown as Page;
  const recovered = await gotoPageWithOneEnvironmentRetry(
    page,
    "http://localhost/settings/security",
  );
  assert.equal(recovered.retried, true);
  assert.equal(calls, 2);

  const forbiddenPage = {
    on() {},
    async goto(): Promise<null> {
      throw new Error("Request failed with status 403");
    },
  } as unknown as Page;
  await assert.rejects(
    () =>
      gotoPageWithOneEnvironmentRetry(
        forbiddenPage,
        "http://localhost/settings/security",
      ),
    /403/,
  );
});

Deno.test(
  "gotoPageWithOneEnvironmentRetry: retries one failed lazy route asset after document load",
  async () => {
    let calls = 0;
    let readyChecks = 0;
    const listeners: Record<string, Array<(value: unknown) => void>> = {};
    const page = {
      on(event: string, listener: (value: unknown) => void) {
        (listeners[event] ??= []).push(listener);
      },
      async goto(): Promise<{ status(): number }> {
        calls++;
        if (calls === 1) {
          for (const listener of listeners.requestfailed ?? []) {
            listener({
              url: () => "http://localhost/_build/assets/security.js",
              failure: () => ({ errorText: "net::ERR_NETWORK_CHANGED" }),
            });
          }
        }
        return { status: () => 200 };
      },
      async content() {
        return "<html><body>This page could not be displayed</body></html>";
      },
    } as unknown as Page;

    const result = await gotoPageWithOneEnvironmentRetry(
      page,
      "http://localhost/settings/security",
      {
        waitForReady: async () => {
          readyChecks++;
          if (readyChecks === 1) {
            throw new Error("account recovery settings did not become visible");
          }
        },
      },
    );

    assert.equal(result.retried, true);
    assert.equal(calls, 2);
    assert.equal(readyChecks, 2);
  },
);

Deno.test(
  "gotoPageWithOneEnvironmentRetry: application readiness failure does not retry",
  async () => {
    let calls = 0;
    const page = {
      async goto(): Promise<{ status(): number }> {
        calls++;
        return { status: () => 200 };
      },
      async content() {
        return "<html><body>Unexpected error</body></html>";
      },
      on() {},
    } as unknown as Page;

    await assert.rejects(
      () =>
        gotoPageWithOneEnvironmentRetry(
          page,
          "http://localhost/settings/security",
          {
            waitForReady: async () => {
              throw new Error(
                "account recovery settings did not become visible",
              );
            },
          },
        ),
      /account recovery settings did not become visible/,
    );
    assert.equal(calls, 1);
  },
);

Deno.test(
  "gotoPageWithOneEnvironmentRetry: ignores failed API requests and stale browser errors",
  async () => {
    let calls = 0;
    const listeners: Record<string, Array<(value: unknown) => void>> = {};
    const page = {
      on(event: string, listener: (value: unknown) => void) {
        (listeners[event] ??= []).push(listener);
      },
      async goto(): Promise<{ status(): number }> {
        calls++;
        for (const listener of listeners.requestfailed ?? []) {
          listener({
            url: () => "http://localhost/api/auth/session",
            failure: () => ({ errorText: "net::ERR_NETWORK_CHANGED" }),
          });
        }
        return { status: () => 200 };
      },
      async content() {
        return "<html><body>Unexpected error</body></html>";
      },
    } as unknown as Page;

    await assert.rejects(
      () =>
        gotoPageWithOneEnvironmentRetry(
          page,
          "http://localhost/settings/security",
          {
            waitForReady: async () => {
              throw new Error(
                "account recovery settings did not become visible; browserErrors=ERR_NETWORK_CHANGED",
              );
            },
          },
        ),
      /browserErrors=ERR_NETWORK_CHANGED/,
    );
    assert.equal(calls, 1);
  },
);
