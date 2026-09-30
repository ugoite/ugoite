import { assertEquals } from "@std/assert/equals";

const checker = new URL("../scripts/ci-gate-check.sh", import.meta.url);

function environment(
  overrides: Record<string, string> = {},
): Record<string, string> {
  return {
    EVENT_NAME: "merge_group",
    IMPACT_RESULT: "success",
    IMPACT_PLAN_STATUS: "ok",
    IMPACT_PLAN_SCOPE: "all",
    IMPACT_JOBS_SKIPPED: "false",
    RUST_CHECK_RESULT: "success",
    RUST_TEST_RESULT: "success",
    S3_SHARED_AUTHORIZATION_RESULT: "success",
    WEB_RESULT: "success",
    ARTIFACT_BUILD_RESULT: "success",
    E2E_SMOKE_MOBILE_RESULT: "success",
    E2E_OWNER_RESULT: "success",
    E2E_PORTABLE_RESULT: "success",
    DOCSITE_NAV_RESULT: "success",
    CP1_FIXTURES_RESULT: "success",
    CP1_QUERY_RESULT: "success",
    CP1_EXPORT_RESULT: "success",
    PLAN_RUST_CHECK: "true",
    PLAN_RUST_TEST: "true",
    PLAN_WEB: "true",
    PLAN_ARTIFACTS: "true",
    PLAN_DOCSITE_NAV: "true",
    PLAN_CP1_ACCEPTANCE: "true",
    PR_CONTEXT_RESULT: "skipped",
    ...overrides,
  };
}

async function check(env: Record<string, string>): Promise<boolean> {
  const command = new Deno.Command("bash", {
    args: [checker.pathname],
    env,
    stdout: "null",
    stderr: "null",
  });
  return (await command.output()).success;
}

Deno.test("required gate accepts all successful lanes and event-specific report skips", async () => {
  assertEquals(await check(environment()), true);
  assertEquals(
    await check(
      environment({ EVENT_NAME: "pull_request", PR_CONTEXT_RESULT: "success" }),
    ),
    true,
  );
});

Deno.test("required gate accepts only the lanes planned for a docs-only pull request", async () => {
  assertEquals(
    await check(environment({
      EVENT_NAME: "pull_request",
      IMPACT_PLAN_SCOPE: "scoped",
      IMPACT_JOBS_SKIPPED: "true",
      RUST_CHECK_RESULT: "skipped",
      RUST_TEST_RESULT: "skipped",
      S3_SHARED_AUTHORIZATION_RESULT: "skipped",
      ARTIFACT_BUILD_RESULT: "skipped",
      E2E_SMOKE_MOBILE_RESULT: "skipped",
      E2E_OWNER_RESULT: "skipped",
      E2E_PORTABLE_RESULT: "skipped",
      CP1_FIXTURES_RESULT: "skipped",
      CP1_QUERY_RESULT: "skipped",
      CP1_EXPORT_RESULT: "skipped",
      PLAN_RUST_CHECK: "false",
      PLAN_RUST_TEST: "false",
      PLAN_ARTIFACTS: "false",
      PLAN_CP1_ACCEPTANCE: "false",
      PR_CONTEXT_RESULT: "success",
    })),
    true,
  );
});

Deno.test("required gate rejects lane failures, cancellation, and invalid plans", async () => {
  const cases: Record<string, string>[] = [
    { RUST_CHECK_RESULT: "failure" },
    { RUST_TEST_RESULT: "cancelled" },
    { S3_SHARED_AUTHORIZATION_RESULT: "failure" },
    { WEB_RESULT: "skipped" },
    { ARTIFACT_BUILD_RESULT: "failure" },
    { E2E_SMOKE_MOBILE_RESULT: "cancelled" },
    { E2E_OWNER_RESULT: "skipped" },
    { E2E_PORTABLE_RESULT: "failure" },
    { DOCSITE_NAV_RESULT: "cancelled" },
    { CP1_FIXTURES_RESULT: "failure" },
    { CP1_QUERY_RESULT: "failure" },
    { CP1_EXPORT_RESULT: "failure" },
    { CP1_FIXTURES_RESULT: "skipped" },
    { CP1_QUERY_RESULT: "cancelled" },
    { CP1_EXPORT_RESULT: "skipped" },
    { EVENT_NAME: "merge_group", CP1_QUERY_RESULT: "skipped" },
    {
      EVENT_NAME: "pull_request",
      IMPACT_PLAN_SCOPE: "scoped",
      IMPACT_JOBS_SKIPPED: "true",
      PLAN_CP1_ACCEPTANCE: "false",
      CP1_FIXTURES_RESULT: "success",
      CP1_QUERY_RESULT: "skipped",
      CP1_EXPORT_RESULT: "skipped",
      RUST_CHECK_RESULT: "skipped",
      RUST_TEST_RESULT: "skipped",
      ARTIFACT_BUILD_RESULT: "skipped",
      E2E_SMOKE_MOBILE_RESULT: "skipped",
      E2E_OWNER_RESULT: "skipped",
      E2E_PORTABLE_RESULT: "skipped",
      PLAN_RUST_CHECK: "false",
      PLAN_RUST_TEST: "false",
      PLAN_ARTIFACTS: "false",
      PR_CONTEXT_RESULT: "success",
    },
    { IMPACT_RESULT: "failure" },
    { IMPACT_PLAN_STATUS: "" },
    { IMPACT_PLAN_SCOPE: "unknown" },
    { IMPACT_JOBS_SKIPPED: "true" },
    { PLAN_RUST_CHECK: "false" },
    { PLAN_ARTIFACTS: "maybe" },
    { PLAN_CP1_ACCEPTANCE: "maybe" },
    { PLAN_WEB: "false", IMPACT_JOBS_SKIPPED: "true" },
    { EVENT_NAME: "merge_group", IMPACT_PLAN_SCOPE: "scoped" },
    {
      EVENT_NAME: "push",
      PLAN_RUST_CHECK: "false",
      IMPACT_JOBS_SKIPPED: "true",
    },
    { EVENT_NAME: "pull_request", PR_CONTEXT_RESULT: "skipped" },
    { EVENT_NAME: "push", PR_CONTEXT_RESULT: "failure" },
  ];
  for (const overrides of cases) {
    assertEquals(
      await check(environment(overrides)),
      false,
      JSON.stringify(overrides),
    );
  }
});
