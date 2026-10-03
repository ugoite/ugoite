import type {
  Browser,
  BrowserContext,
  CDPSession,
  Page,
} from "@playwright/test";
import {
  addVirtualAuthenticator,
  removeVirtualAuthenticator,
} from "./webauthn.ts";

export type IsolatedPasskeyPage = {
  target: BrowserContext;
  page: Page;
  /**
   * CDP session with WebAuthn enabled, for credential assertions
   * (`WebAuthn.getCredentials`) on long journeys. The session belongs to
   * `target`; `close` disables WebAuthn, so callers must not close it.
   */
  cdp: CDPSession;
  /** Virtual authenticator owned by this page; removed by `close`. */
  authenticatorId: string;
  /** Tears down the authenticator, the WebAuthn session, and the context. */
  close: () => Promise<void>;
};

/**
 * Opens a security-ceremony page on fully isolated browser state.
 *
 * Passkey, invitation, OIDC, and recovery journeys must not inherit
 * authenticator or event-loop state from prior tests: the context starts
 * with empty cookies/origins, gets exactly one virtual authenticator, and
 * `close` removes that authenticator and disables WebAuthn before closing
 * the context, even when the journey itself failed. Teardown steps are
 * best-effort so one cleanup failure never masks the journey result.
 *
 * Long journeys that need credential assertions must use this helper (via
 * the exposed `cdp`/`authenticatorId`) instead of a hand-rolled context so
 * every ceremony path gets identical teardown parity.
 */
export async function openIsolatedPasskeyPage(
  browser: Browser,
): Promise<IsolatedPasskeyPage> {
  const target = await browser.newContext({
    storageState: { cookies: [], origins: [] },
  });
  const page = await target.newPage();
  const cdp = await target.newCDPSession(page);
  await cdp.send("WebAuthn.enable");
  const authenticatorId = await addVirtualAuthenticator(cdp);
  let closed = false;
  const close = async (): Promise<void> => {
    if (closed) {
      return;
    }
    closed = true;
    try {
      await removeVirtualAuthenticator(cdp, authenticatorId);
    } catch {
      // Best-effort: teardown must not mask the journey result.
    }
    try {
      await cdp.send("WebAuthn.disable");
    } catch {
      // Best-effort: teardown must not mask the journey result.
    }
    await target.close();
  };
  return { target, page, cdp, authenticatorId, close };
}

const ENVIRONMENT_FAILURE_PATTERNS = [
  "ERR_NETWORK_CHANGED",
  "ERR_INTERNET_DISCONNECTED",
  "ERR_CONNECTION_REFUSED",
  "ERR_CONNECTION_RESET",
  "ERR_CONNECTION_CLOSED",
  "ERR_NAME_NOT_RESOLVED",
  "ERR_ADDRESS_UNREACHABLE",
  "NS_ERROR_",
  "ECONNREFUSED",
  "ECONNRESET",
  "ENOTFOUND",
  "EAI_AGAIN",
  "net::",
];

/**
 * Product verdicts that must NEVER trigger a context rebuild, even when the
 * message happens to mention a network token. Checked before the environment
 * allowlist so retry fails closed: HTTP verdicts, API validation/authorization
 * errors, WebAuthn ceremony failures, visible application errors, assertion
 * failures, and product state mismatches.
 */
const NEVER_RETRY_PATTERNS = [
  "returned 4",
  "returned 5",
  "status: 4",
  "status: 5",
  "HTTP 4",
  "HTTP 5",
  " 400",
  " 401",
  " 403",
  " 404",
  " 409",
  " 422",
  " 423",
  " 500",
  " 502",
  " 503",
  "ORIGIN_MISMATCH",
  "NODE_UNINITIALIZED",
  "VALIDATION",
  "validation",
  "Unauthorized",
  "Forbidden",
  "NotAllowedError",
  "InvalidStateError",
  "SecurityError",
  "WebAuthn",
  "webauthn",
  "authenticator",
  "passkey",
  "ceremony",
  "toBe",
  "toHave",
  "toMatch",
  "toEqual",
  "expect(",
  "[application]",
];

/** Static frontend chunk/asset failure markers (/_build/ JS, manifest). */
const STATIC_ASSET_PATTERNS = [
  "/_build/",
  "ugoite-manifest.js",
  "Loading chunk",
  "Loading CSS chunk",
  "Dynamically imported module",
  "Failed to fetch dynamically imported module",
  "failed to load resource",
  "modulepreload",
];

const SAFE_ERROR_NAMES = new Set([
  "AbortError",
  "ConstraintError",
  "DataError",
  "Error",
  "EvalError",
  "NetworkError",
  "NotAllowedError",
  "NotFoundError",
  "OperationError",
  "RangeError",
  "ReferenceError",
  "SecurityError",
  "SyntaxError",
  "TimeoutError",
  "TypeError",
  "URIError",
]);

const SAFE_NETWORK_FAILURE_CODES = new Set([
  "ERR_ABORTED",
  "ERR_ADDRESS_UNREACHABLE",
  "ERR_BLOCKED_BY_CLIENT",
  "ERR_CONNECTION_CLOSED",
  "ERR_CONNECTION_REFUSED",
  "ERR_CONNECTION_RESET",
  "ERR_CONNECTION_TIMED_OUT",
  "ERR_FAILED",
  "ERR_INTERNET_DISCONNECTED",
  "ERR_NAME_NOT_RESOLVED",
  "ERR_NETWORK_CHANGED",
  "ERR_NETWORK_IO_SUSPENDED",
  "ERR_TIMED_OUT",
  "NS_ERROR_CONNECTION_REFUSED",
  "NS_ERROR_NET_RESET",
  "NS_ERROR_NET_TIMEOUT",
  "NS_ERROR_UNKNOWN_HOST",
  "ECONNREFUSED",
  "ECONNRESET",
  "ECONNCLOSED",
  "ENOTFOUND",
  "EAI_AGAIN",
]);

function messageOf(error: unknown): string {
  return error instanceof Error
    ? `${error.message}\n${error.stack ?? ""}`
    : String(error);
}

/** Keep error-name diagnostics to platform categories; custom names may contain secrets. */
export function safeErrorName(name: string): string {
  return SAFE_ERROR_NAMES.has(name) ? name : "Error";
}

/** Map browser request failures to a fixed category rather than logging their text. */
export function safeNetworkFailureCode(value: string | undefined): string {
  const candidate = value?.match(/(?:^|[^A-Z0-9_])([A-Z][A-Z0-9_]+)(?:$|[^A-Z0-9_])/)
    ?.[1];
  return candidate && SAFE_NETWORK_FAILURE_CODES.has(candidate)
    ? candidate
    : "request-failed";
}

/** Return a bounded failure category suitable for logs that contain secrets. */
export function safeNavigationFailure(error: unknown): string {
  const message = messageOf(error);
  const status = message.match(/\b(?:returned\s+|status:?\s*)([45]\d\d)\b/i)?.[1];
  if (status) return `http-${status}`;

  // Playwright's navigation text places the request URL after this prefix;
  // never scan URL paths, queries, or fragments for a diagnostic category.
  const errorPrefix = message.split(/https?:\/\//, 1)[0];
  const code = [...SAFE_NETWORK_FAILURE_CODES].find((candidate) =>
    errorPrefix.includes(candidate)
  );
  if (code) return code;

  if (/timeout/i.test(message)) return "timeout";
  const ceremonyError = message.match(
    /\b(?:NotAllowedError|InvalidStateError|SecurityError|AbortError|ConstraintError)\b/,
  )?.[0];
  if (ceremonyError) return ceremonyError;
  if (/did not become visible|readiness check failed/i.test(message)) {
    return "readiness-failed";
  }
  if (error instanceof Error && safeErrorName(error.name) !== "Error") {
    return safeErrorName(error.name);
  }
  return "navigation-error";
}

/** True when a failure is harness/environment noise, not an app verdict. */
export function isEnvironmentFailure(error: unknown): boolean {
  const message = messageOf(error);
  return ENVIRONMENT_FAILURE_PATTERNS.some((pattern) =>
    message.includes(pattern)
  );
}

/** True when a failure is a product verdict that must never be retried. */
export function isProductFailure(error: unknown): boolean {
  const message = messageOf(error);
  return NEVER_RETRY_PATTERNS.some((pattern) => message.includes(pattern));
}

/** True when the error text shows a static JS/CSS chunk request failing. */
export function isStaticAssetFailure(error: unknown): boolean {
  const message = messageOf(error);
  return STATIC_ASSET_PATTERNS.some((pattern) => message.includes(pattern));
}

export type DocumentProbe = {
  /** HTTP status of the document response, when known. */
  status?: number;
  /** Snippet of the loaded document body, when known. */
  bodySnippet?: string;
  /** Recorded failed asset/chunk request text, when known. */
  assetError?: string;
};

/**
 * Pure retry verdict for a navigation failure. Returns `"retry"` ONLY for:
 * document load failure, recognized ERR_NETWORK_CHANGED, connection
 * reset/refused, static JS chunk request failure, or a 2xx document with an
 * empty DOM plus a frontend asset environment error. Every product verdict
 * (HTTP 4xx/5xx, API validation/authorization, WebAuthn ceremony, visible
 * application error, assertion failure, product state mismatch) returns
 * `"no-retry"`. Product signals take precedence so retry fails closed.
 */
export function classifyNavigationFailure(
  error: unknown,
  probe: DocumentProbe = {},
): "retry" | "no-retry" {
  if (
    typeof probe.status === "number" && probe.status >= 400 &&
    probe.status <= 599
  ) {
    return "no-retry";
  }
  if (isProductFailure(error)) return "no-retry";
  if (isEnvironmentFailure(error)) return "retry";
  if (
    isStaticAssetFailure(error) && isEnvironmentFailure(probe.assetError ?? "")
  ) {
    return "retry";
  }
  if (isStaticAssetFailure(probe.assetError ?? "")) return "retry";
  const body = probe.bodySnippet ?? "";
  const emptyDom = body.length === 0 ||
    (!body.includes('<div id="app"') && !body.includes("/_build/"));
  if (
    emptyDom &&
    (isEnvironmentFailure(probe.assetError ?? "") ||
      isStaticAssetFailure(probe.assetError ?? ""))
  ) {
    return "retry";
  }
  return "no-retry";
}

/** True only when the failure earns exactly one environment rebuild. */
export function shouldRetryEnvironmentFailure(
  error: unknown,
  probe: DocumentProbe = {},
): boolean {
  return classifyNavigationFailure(error, probe) === "retry";
}

/**
 * Prefixes a ceremony failure for the test report so environment noise
 * (hopper network flaps, DNS, refused sockets) never masquerades as a
 * product verdict: `[environment]` vs `[application]`.
 */
export function describeFailure(error: unknown, ceremony: string): Error {
  const prefix = isEnvironmentFailure(error) ? "environment" : "application";
  const message = error instanceof Error ? error.message : String(error);
  const described = new Error(`[${prefix}] ${ceremony}: ${message}`);
  if (error instanceof Error && error.stack) {
    described.stack = error.stack;
  }
  return described;
}
