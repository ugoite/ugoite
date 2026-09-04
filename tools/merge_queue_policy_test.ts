import { assertEquals } from "@std/assert/equals";
import { isDependabotPullRequest } from "./merge_queue_policy.ts";

Deno.test("merge-queue policy identifies Dependabot pull requests", () => {
  assertEquals(
    isDependabotPullRequest({ user: { login: "dependabot[bot]" } }),
    true,
  );
  assertEquals(
    isDependabotPullRequest({ user: { login: "human-contributor" } }),
    false,
  );
  assertEquals(isDependabotPullRequest(undefined), false);
});
