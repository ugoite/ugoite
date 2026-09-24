import type {
  Browser,
  BrowserContext,
  BrowserContextOptions,
  Page,
} from "@playwright/test";
import {
  classifyNavigationFailure,
  type DocumentProbe,
} from "./security-context.ts";

export type NavigationRetryOptions = {
  /** Human label used in the retry log line. Defaults to the URL. */
  label?: string;
  /** Options for each freshly created browser context. */
  contextOptions?: BrowserContextOptions;
  /** Options forwarded to `page.goto`. */
  gotoOptions?: Parameters<Page["goto"]>[1];
  /**
   * Runs on every fresh page before navigation (for example enabling
   * WebAuthn). Navigation-scoped setup only: never perform product API
   * mutations here, they are not retried and must not be repeated.
   */
  prepare?: (page: Page, target: BrowserContext) => Promise<void>;
  /**
   * Readiness check after navigation (for example waiting for a heading).
   * Failures here retry only for a 2xx document with an empty DOM plus a
   * frontend asset environment error; every other post-load failure is an
   * application verdict and throws without retrying.
   */
  waitForReady?: (page: Page) => Promise<void>;
  /**
   * When true, a navigation that returns a non-OK response throws a product
   * error without retrying. HTTP verdicts are never environment failures.
   */
  requireOkResponse?: boolean;
};

export type NavigationRetryResult = {
  target: BrowserContext;
  page: Page;
  /** True when the first context was discarded after an environment failure. */
  retried: boolean;
};

/**
 * Navigates a fresh browser context to `url`, allowing exactly ONE context
 * rebuild when the initial `page.goto` itself throws a browser-level
 * environment failure (document load failure, recognized
 * `ERR_NETWORK_CHANGED`, refused/reset sockets, static JS chunk request
 * failure, as classified by `classifyNavigationFailure`).
 *
 * A post-navigation readiness failure retries only for the narrow 2xx
 * document with an empty DOM plus a frontend asset environment error: the
 * page already loaded, so any other readiness failure is an application
 * verdict and a new context cannot help. HTTP 4xx/5xx, API validation or
 * authorization errors, WebAuthn ceremony failures, visible application
 * errors, assertion failures, and product state mismatches never retry.
 *
 * Code-guard against retrying product work: this helper can only perform
 * navigation. It accepts no generic callback, so product API calls cannot be
 * wrapped in it; non-OK responses and post-navigation readiness failures
 * always throw without retrying unless the empty-DOM asset rule matches.
 * Callers must only use it from setup/navigation paths (global setup,
 * initial ceremony navigation).
 */
export async function gotoWithOneEnvironmentRetry(
  browser: Browser,
  url: string,
  options: NavigationRetryOptions = {},
): Promise<NavigationRetryResult> {
  const label = options.label ?? url;

  const attempt = async (): Promise<
    {
      target: BrowserContext;
      page: Page;
      status?: number;
      assetErrors: string[];
    }
  > => {
    const target = await browser.newContext({
      storageState: { cookies: [], origins: [] },
      ...options.contextOptions,
    });
    const page = await target.newPage();
    const assetErrors: string[] = [];
    page.on("requestfailed", (request) => {
      const failure = request.failure()?.errorText ?? "requestfailed";
      assetErrors.push(`${request.url()} :: ${failure}`);
    });
    page.on("console", (message) => {
      if (message.type() === "error") assetErrors.push(message.text());
    });
    let status: number | undefined;
    try {
      await options.prepare?.(page, target);
      const response = await page.goto(url, options.gotoOptions);
      status = response?.status();
      if (options.requireOkResponse && !response?.ok()) {
        throw new Error(
          `navigation to ${url} returned ${response?.status()}: ${
            (await response?.text())?.slice(0, 2000)
          }`,
        );
      }
    } catch (error) {
      await target.close().catch(() => {});
      throw error;
    }
    return { target, page, status, assetErrors };
  };

  const probeForReadyFailure = async (
    entry: { page: Page; status?: number; assetErrors: string[] },
  ): Promise<DocumentProbe> => {
    const bodySnippet = await entry.page.content().then(
      (html) => html.slice(0, 2000),
      () => "",
    );
    return {
      status: entry.status,
      bodySnippet,
      assetError: entry.assetErrors.join(" | ").slice(0, 2000),
    };
  };

  try {
    const first = await attempt();
    try {
      await options.waitForReady?.(first.page);
    } catch (error) {
      const probe = await probeForReadyFailure(first);
      await first.target.close().catch(() => {});
      // Narrow exception: a 2xx document with an empty DOM plus a frontend
      // asset environment error earns the single rebuild. Everything else
      // that fails after load is an application verdict.
      if (classifyNavigationFailure(error, probe) === "retry") {
        const reason = error instanceof Error ? error.message : String(error);
        console.log(
          `[environment] ${label}: ready check hit an empty document with an asset failure; rebuilding the browser context once: ${
            reason.slice(0, 500)
          }`,
        );
        const second = await attempt();
        try {
          await options.waitForReady?.(second.page);
        } catch (readyError) {
          await second.target.close().catch(() => {});
          throw readyError;
        }
        return { target: second.target, page: second.page, retried: true };
      }
      throw error;
    }
    return { target: first.target, page: first.page, retried: false };
  } catch (error) {
    if (classifyNavigationFailure(error) !== "retry") throw error;
    const reason = error instanceof Error ? error.message : String(error);
    console.log(
      `[environment] ${label}: initial navigation hit a browser-level failure; rebuilding the browser context once: ${
        reason.slice(0, 500)
      }`,
    );
    // The second attempt throws through: exactly one rebuild is allowed.
    const second = await attempt();
    try {
      await options.waitForReady?.(second.page);
    } catch (readyError) {
      await second.target.close().catch(() => {});
      throw readyError;
    }
    return { target: second.target, page: second.page, retried: true };
  }
}

/**
 * Same environment classification for navigation on an EXISTING page (for
 * example the authenticated /settings/security hop inside global setup,
 * where rebuilding the context would drop the fresh session). Retries the
 * `page.goto` exactly once when `classifyNavigationFailure` says retry;
 * product verdicts throw on the first attempt.
 */
export async function gotoPageWithOneEnvironmentRetry(
  page: Page,
  url: string,
  options: {
    label?: string;
    gotoOptions?: Parameters<Page["goto"]>[1];
    waitForReady?: (page: Page) => Promise<void>;
  } = {},
): Promise<{ retried: boolean }> {
  const label = options.label ?? url;
  const assetErrors: string[] = [];
  page.on("requestfailed", (request) => {
    const failure = request.failure()?.errorText ?? "requestfailed";
    assetErrors.push(`${request.url()} :: ${failure}`);
  });
  page.on("console", (message) => {
    if (message.type() === "error") assetErrors.push(message.text());
  });

  const probe = async (status?: number) => ({
    status,
    bodySnippet: await page.content().then((html) => html.slice(0, 2000), () => ""),
    assetError: assetErrors.join(" | ").slice(0, 2000),
  });
  const waitForReady = async () => {
    await options.waitForReady?.(page);
  };

  let status: number | undefined;
  try {
    status = (await page.goto(url, options.gotoOptions))?.status();
  } catch (error) {
    if (classifyNavigationFailure(error) !== "retry") throw error;
    const reason = error instanceof Error ? error.message : String(error);
    console.log(
      `[environment] ${label}: navigation hit a browser-level failure; retrying once on the same page: ${
        reason.slice(0, 500)
      }`,
    );
    status = (await page.goto(url, options.gotoOptions))?.status();
    await waitForReady();
    return { retried: true };
  }

  try {
    await waitForReady();
    return { retried: false };
  } catch (error) {
    if (classifyNavigationFailure(error, await probe(status)) !== "retry") {
      throw error;
    }
    const reason = error instanceof Error ? error.message : String(error);
    console.log(
      `[environment] ${label}: ready check hit a failed frontend asset; retrying once on the same page: ${
        reason.slice(0, 500)
      }`,
    );
    status = (await page.goto(url, options.gotoOptions))?.status();
    await waitForReady();
    return { retried: true };
  }
}
