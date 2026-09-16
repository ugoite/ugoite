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

/** True when a failure is harness/environment noise, not an app verdict. */
export function isEnvironmentFailure(error: unknown): boolean {
  const message = error instanceof Error
    ? `${error.message}\n${error.stack ?? ""}`
    : String(error);
  return ENVIRONMENT_FAILURE_PATTERNS.some((pattern) =>
    message.includes(pattern)
  );
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
