import type { Page } from "@playwright/test";
import { safeErrorName, safeNetworkFailureCode } from "./security-context.ts";

const MAX_DIAGNOSTIC_ITEMS = 5;

export type SetupReadinessDiagnostics = {
  documentResponses: string[];
  navigationFailures: string[];
  assetFailures: string[];
  pageErrors: string[];
  consoleErrors: string[];
  navigationAttempts: number;
  domSnapshot?: SafeDomSnapshot;
};

export type SafeDomSnapshot = {
  readyState: string;
  bodyElementCount: number;
  bodyTextLength: number;
};

const SAFE_DIAGNOSTIC_PATHS = new Set([
  "/",
  "/setup",
  "/login",
  "/spaces",
  "/settings/security",
  "/ugoite-manifest.js",
]);

export function createSetupReadinessDiagnostics(): SetupReadinessDiagnostics {
  return {
    documentResponses: [],
    navigationFailures: [],
    assetFailures: [],
    pageErrors: [],
    consoleErrors: [],
    navigationAttempts: 0,
  };
}

export function safeUrlPath(value: string): string {
  try {
    const path = new URL(value).pathname || "/";
    if (SAFE_DIAGNOSTIC_PATHS.has(path)) return path;

    const assetExtension = path.match(/^\/_build\/.+\.(m?js|css)$/)?.[1];
    if (assetExtension) return `/_build/<asset>.${assetExtension}`;
    if (path.startsWith("/_build/")) return "/_build/<asset>";
    return "<path>";
  } catch {
    return "<unavailable>";
  }
}

function remember(target: string[], value: string): void {
  if (!target.includes(value)) target.push(value);
  if (target.length > MAX_DIAGNOSTIC_ITEMS) target.shift();
}

function isFrontendAsset(path: string): boolean {
  return path === "/ugoite-manifest.js" ||
    (path.startsWith("/_build/") && /\.(?:m?js|css)$/.test(path));
}

function failureCode(value: string | undefined): string {
  return safeNetworkFailureCode(value);
}

function isMainNavigation(page: Page, request: {
  isNavigationRequest(): boolean;
  frame(): unknown;
}): boolean {
  try {
    return request.isNavigationRequest() &&
      request.frame() === page.mainFrame();
  } catch {
    return false;
  }
}

/** Attach before navigation so a readiness timeout includes the document and asset outcomes. */
export function observeSetupReadiness(
  page: Page,
  diagnostics: SetupReadinessDiagnostics,
): void {
  const attempt = ++diagnostics.navigationAttempts;
  page.on("response", (response) => {
    const request = response.request();
    const path = safeUrlPath(response.url());
    if (isMainNavigation(page, request)) {
      remember(
        diagnostics.documentResponses,
        `attempt-${attempt}:${path}:http-${response.status()}`,
      );
    } else if (isFrontendAsset(path) && !response.ok()) {
      remember(
        diagnostics.assetFailures,
        `attempt-${attempt}:${path}:http-${response.status()}`,
      );
    }
  });

  page.on("requestfailed", (request) => {
    const path = safeUrlPath(request.url());
    const code = failureCode(request.failure()?.errorText);
    if (isMainNavigation(page, request)) {
      remember(
        diagnostics.navigationFailures,
        `attempt-${attempt}:${path}:${code}`,
      );
    } else if (isFrontendAsset(path)) {
      remember(diagnostics.assetFailures, `attempt-${attempt}:${path}:${code}`);
    }
  });

  page.on("pageerror", (error) => {
    remember(
      diagnostics.pageErrors,
      `attempt-${attempt}:${safeErrorName(error.name)}`,
    );
  });

  page.on("console", (message) => {
    if (message.type() === "error") {
      remember(
        diagnostics.consoleErrors,
        `attempt-${attempt}:${safeUrlPath(message.location().url)}`,
      );
    }
  });
}

export function formatSetupReadinessDiagnostics(
  url: string,
  diagnostics: SetupReadinessDiagnostics,
  dom?: SafeDomSnapshot,
): string {
  const values = [
    `path=${safeUrlPath(url)}`,
    `documentResponses=${diagnostics.documentResponses.join(",") || "none"}`,
    `navigationFailures=${diagnostics.navigationFailures.join(",") || "none"}`,
    `assetFailures=${diagnostics.assetFailures.join(",") || "none"}`,
    `pageErrors=${diagnostics.pageErrors.join(",") || "none"}`,
    `consoleErrors=${diagnostics.consoleErrors.join(",") || "none"}`,
    `navigationAttempts=${diagnostics.navigationAttempts}`,
  ];
  const snapshot = dom ?? diagnostics.domSnapshot;
  if (snapshot) {
    values.push(
      `domReadyState=${snapshot.readyState}`,
      `bodyElementCount=${snapshot.bodyElementCount}`,
      `bodyTextLength=${snapshot.bodyTextLength}`,
    );
  }
  return values.join("; ");
}

export async function snapshotSetupDom(page: Page): Promise<SafeDomSnapshot> {
  try {
    return await page.evaluate(() => ({
      readyState: document.readyState,
      bodyElementCount: document.body?.childElementCount ?? 0,
      bodyTextLength: document.body?.innerText.length ?? 0,
    }));
  } catch {
    return {
      readyState: "unavailable",
      bodyElementCount: 0,
      bodyTextLength: 0,
    };
  }
}

/** Preserve the safe DOM summary for the outer setup failure formatter. */
export async function recordSetupDomSnapshot(
  page: Page,
  diagnostics: SetupReadinessDiagnostics,
): Promise<SafeDomSnapshot> {
  const snapshot = await snapshotSetupDom(page);
  diagnostics.domSnapshot = snapshot;
  return snapshot;
}
