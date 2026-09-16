import type { Page } from "@playwright/test";

/**
 * Ceremony step logging and longtask observation for E2E recovery/OIDC
 * journeys. These helpers are log-only harness instrumentation: they never
 * change product behavior, never assert, and never retry. Timeouts and
 * verdicts stay owned by the calling test's existing expectations.
 */

/** Emits one `[ceremony]` line to test output so journey stalls are visible. */
export function logCeremonyStep(
  ceremony: string,
  step: string,
  detail = "",
): void {
  const suffix = detail ? ` ${detail}` : "";
  console.log(`[ceremony] ${ceremony}: ${step}${suffix}`);
}

const LONGTASK_STORE = "__ugoiteE2eLongtasks";

/**
 * Installs a `longtask` PerformanceObserver before navigation. The observer
 * only records durations on the page; reporting happens via
 * `reportLongtasks`. Best-effort: pages without `longtask` support stay
 * silent. Implemented with string scripts so no DOM lib types are needed.
 */
export async function installLongtaskObserver(page: Page): Promise<void> {
  await page.addInitScript(
    `try {
      const store = (globalThis[${JSON.stringify(LONGTASK_STORE)}] ??= []);
      const observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) store.push(entry.duration);
      });
      observer.observe({ entryTypes: ["longtask"] });
    } catch {
      // Log-only instrumentation must never break a ceremony.
    }`,
  );
}

/**
 * Reads recorded longtask durations and logs a one-line summary to test
 * output. Never throws: teardown paths call this after failures where the
 * page may already be closed.
 */
export async function reportLongtasks(
  page: Page,
  ceremony: string,
  step: string,
): Promise<void> {
  let durations: unknown = [];
  try {
    durations = await page.evaluate(
      `(globalThis[${JSON.stringify(LONGTASK_STORE)}] ?? [])`,
    );
  } catch {
    return;
  }
  if (!Array.isArray(durations) || durations.length === 0) return;
  const samples = durations.filter((value): value is number =>
    typeof value === "number"
  );
  if (samples.length === 0) return;
  console.log(
    `[ceremony] ${ceremony}: ${step} longtask count=${samples.length} max=${
      Math.round(Math.max(...samples))
    }ms`,
  );
}
