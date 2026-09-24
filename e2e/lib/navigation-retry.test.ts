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
  shouldRetryEnvironmentFailure,
} from "./security-context.ts";
import {
  gotoPageWithOneEnvironmentRetry,
  gotoWithOneEnvironmentRetry,
} from "./navigation-retry.ts";

const NETWORK_CHANGED = new Error(
  "page.goto: net::ERR_NETWORK_CHANGED at https://localhost/setup",
);

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
      bodySnippet: "<html><head></head><body></body></html>",
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
      bodySnippet: "<html><head></head><body></body></html>",
    }),
    "no-retry",
  );
});

// --- Helper-level harness with fake Playwright objects. ---

type FakeGoto = (
  url: string,
) => Promise<
  { status(): number; ok(): boolean; text(): Promise<string> } | null
>;

function fakeBrowser(gotos: FakeGoto[], hooks?: { onReadyFail?: Error }): {
  browser: Browser;
  contextsCreated: () => number;
} {
  let contexts = 0;
  let gotoCalls = 0;
  const browser = {
    async newContext(): Promise<BrowserContext> {
      contexts++;
      const listeners: Record<string, Array<(...args: never[]) => void>> = {};
      const page = {
        on(event: string, listener: (...args: never[]) => void) {
          (listeners[event] ??= []).push(listener);
        },
        async goto(url: string) {
          const behavior = gotos[Math.min(gotoCalls++, gotos.length - 1)];
          return await behavior(url);
        },
        async content(): Promise<string> {
          return "<html><head></head><body></body></html>";
        },
      } as unknown as Page;
      return {
        async newPage(): Promise<Page> {
          return page;
        },
        async close(): Promise<void> {},
      } as unknown as BrowserContext;
    },
  } as unknown as Browser;
  void hooks;
  return { browser, contextsCreated: () => contexts };
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
              throw new Error("account recovery settings did not become visible");
            },
          },
        ),
      /account recovery settings did not become visible/,
    );
    assert.equal(calls, 1);
  },
);
