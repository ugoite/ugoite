export function isDependabotPullRequest(
  pullRequest: { user?: { login?: string | null } } | null | undefined,
): boolean {
  return pullRequest?.user?.login === "dependabot[bot]";
}
