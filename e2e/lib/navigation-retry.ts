import type {
  Browser,
  BrowserContext,
  BrowserContextOptions,
  Page,
} from "@playwright/test";
import {
  classifyNavigationFailure,
  type DocumentProbe,
  isEnvironmentFailure,
  isProductFailure,
  safeNavigationFailure,
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
   * A failed frontend script request can interrupt this check immediately.
   * Only transient script network failures retry; HTTP asset failures and
   * every unrelated readiness failure throw without retrying.
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

function isFrontendAsset(url: string): boolean {
  try {
    const path = new URL(url).pathname;
    return path === "/ugoite-manifest.js" ||
      (path.startsWith("/_build/") && /\.m?js$/.test(path));
  } catch {
    return false;
  }
}

async function visibleBodyText(page: Page): Promise<string> {
  try {
    return await page.evaluate(() => document.body?.innerText ?? "");
  } catch {
    return "";
  }
}

/**
 * Navigates a fresh browser context to `url`, allowing exactly ONE context
 * rebuild when the initial `page.goto` itself throws a browser-level
 * environment failure (document load failure, recognized
 * `ERR_NETWORK_CHANGED`, refused/reset sockets, static JS chunk request
 * failure, as classified by `classifyNavigationFailure`).
 *
 * A post-navigation readiness failure retries only when a frontend script
 * failed with a transient network error. This also covers server-rendered
 * setup markup whose JavaScript did not hydrate. Persistent HTTP asset errors,
 * HTTP 4xx/5xx, API validation or authorization errors, WebAuthn ceremony
 * failures, assertion failures, and product state mismatches never retry.
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
      readinessFailure: Promise<void>;
      readinessFailed: () => boolean;
    }
  > => {
    const target = await browser.newContext({
      storageState: { cookies: [], origins: [] },
      ...options.contextOptions,
    });
    const page = await target.newPage();
    const assetErrors: string[] = [];
    let readinessFailed = false;
    let signalReadinessFailure!: () => void;
    const readinessFailure = new Promise<void>((resolve) => {
      signalReadinessFailure = resolve;
    });
    const markReadinessFailure = () => {
      readinessFailed = true;
      signalReadinessFailure();
    };
    page.on("requestfailed", (request) => {
      if (!isFrontendAsset(request.url())) return;
      const failure = request.failure()?.errorText ?? "requestfailed";
      assetErrors.push(`${request.url()} :: ${failure}`);
      markReadinessFailure();
    });
    page.on("response", (response) => {
      if (isFrontendAsset(response.url()) && !response.ok()) {
        assetErrors.push(`${response.url()} :: http-${response.status()}`);
        markReadinessFailure();
      }
    });
    page.on("pageerror", () => {
      assetErrors.push("frontend-page-error");
      markReadinessFailure();
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
    return {
      target,
      page,
      status,
      assetErrors,
      readinessFailure,
      readinessFailed: () => readinessFailed,
    };
  };

  const probeForReadyFailure = async (
    entry: { page: Page; status?: number; assetErrors: string[] },
  ): Promise<DocumentProbe> => {
    return {
      status: entry.status,
      bodyText: (await visibleBodyText(entry.page)).slice(0, 2000),
      assetError: entry.assetErrors.join(" | ").slice(0, 2000),
    };
  };

  const waitForReady = async (
    entry: Awaited<ReturnType<typeof attempt>>,
  ): Promise<void> => {
    if (!options.waitForReady) return;
    if (entry.readinessFailed()) {
      throw new Error("readiness check failed after a frontend load error");
    }
    await Promise.race([
      options.waitForReady(entry.page),
      entry.readinessFailure.then(() => {
        throw new Error(
          "readiness check failed after a frontend load error",
        );
      }),
    ]);
  };

  try {
    const first = await attempt();
    try {
      await waitForReady(first);
    } catch (error) {
      const probe = await probeForReadyFailure(first);
      await first.target.close().catch(() => {});
      // Retry only when the probe identifies a transient frontend asset
      // network failure. Missing/invalid assets and product failures fail
      // without rebuilding the context.
      if (
        !isProductFailure(error) &&
        classifyNavigationFailure("readiness check failed", probe) === "retry"
      ) {
        const reason = safeNavigationFailure(error);
        console.log(
          `[environment] ${label}: ready check observed a frontend asset failure; rebuilding the browser context once: ${reason}`,
        );
        const second = await attempt();
        try {
          await waitForReady(second);
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
    const reason = safeNavigationFailure(error);
    console.log(
      `[environment] ${label}: initial navigation hit a browser-level failure; rebuilding the browser context once: ${reason}`,
    );
    // The second attempt throws through: exactly one rebuild is allowed.
    const second = await attempt();
    try {
      await waitForReady(second);
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
  let observingNavigation = false;
  page.on("requestfailed", (request) => {
    if (!observingNavigation) return;
    const pathname = new URL(request.url()).pathname;
    const isFrontendAsset =
      (pathname.startsWith("/_build/") && /\.(?:m?js|css)$/.test(pathname)) ||
      pathname.endsWith("/ugoite-manifest.js");
    const failure = request.failure()?.errorText ?? "requestfailed";
    if (isFrontendAsset && isEnvironmentFailure(failure)) {
      assetErrors.push(`${request.url()} :: ${failure}`);
    }
  });

  const probe = async (status?: number) => ({
    status,
    bodySnippet: await page.content().then(
      (html) => html.slice(0, 2000),
      () => "",
    ),
    assetError: assetErrors.join(" | ").slice(0, 2000),
  });
  const waitForReady = async () => {
    try {
      await options.waitForReady?.(page);
    } finally {
      observingNavigation = false;
    }
  };

  let status: number | undefined;
  observingNavigation = true;
  try {
    status = (await page.goto(url, options.gotoOptions))?.status();
  } catch (error) {
    observingNavigation = false;
    if (classifyNavigationFailure(error) !== "retry") throw error;
    const reason = safeNavigationFailure(error);
    console.log(
      `[environment] ${label}: navigation hit a browser-level failure; retrying once on the same page: ${reason}`,
    );
    assetErrors.length = 0;
    observingNavigation = true;
    await page.goto(url, options.gotoOptions);
    await waitForReady();
    return { retried: true };
  }

  try {
    await waitForReady();
    return { retried: false };
  } catch (error) {
    const navigationProbe = await probe(status);
    if (
      classifyNavigationFailure("readiness check failed", navigationProbe) !==
        "retry"
    ) {
      throw error;
    }
    const reason = safeNavigationFailure(error);
    console.log(
      `[environment] ${label}: ready check hit a failed frontend asset; retrying once on the same page: ${reason}`,
    );
    assetErrors.length = 0;
    observingNavigation = true;
    await page.goto(url, options.gotoOptions);
    await waitForReady();
    return { retried: true };
  }
}
