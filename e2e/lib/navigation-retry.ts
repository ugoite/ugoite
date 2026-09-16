import type {
  Browser,
  BrowserContext,
  BrowserContextOptions,
  Page,
} from "@playwright/test";
import { isEnvironmentFailure } from "./security-context.ts";

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
   * Failures here never trigger a rebuild: the page already loaded, so a
   * new context cannot help and retrying would mask an application verdict.
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
 * environment failure (`ERR_NETWORK_CHANGED`, refused sockets, DNS, ... as
 * classified by `isEnvironmentFailure`).
 *
 * Code-guard against retrying product work: this helper can only perform
 * navigation. It accepts no generic callback, so product API calls cannot be
 * wrapped in it; non-OK responses and post-navigation readiness failures
 * always throw without retrying. Callers must only use it from
 * setup/navigation paths (global setup, initial ceremony navigation).
 */
export async function gotoWithOneEnvironmentRetry(
  browser: Browser,
  url: string,
  options: NavigationRetryOptions = {},
): Promise<NavigationRetryResult> {
  const label = options.label ?? url;

  const attempt = async (): Promise<
    { target: BrowserContext; page: Page }
  > => {
    const target = await browser.newContext({
      storageState: { cookies: [], origins: [] },
      ...options.contextOptions,
    });
    const page = await target.newPage();
    try {
      await options.prepare?.(page, target);
      const response = await page.goto(url, options.gotoOptions);
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
    return { target, page };
  };

  try {
    const first = await attempt();
    try {
      await options.waitForReady?.(first.page);
    } catch (error) {
      await first.target.close().catch(() => {});
      throw error;
    }
    return { ...first, retried: false };
  } catch (error) {
    if (!isEnvironmentFailure(error)) throw error;
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
    return { ...second, retried: true };
  }
}
