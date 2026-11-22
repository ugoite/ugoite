import { validateKnowledgeCompatibilityReview } from "./knowledge_compatibility.ts";

export const requiredSections = [
  "## Summary",
  "## Related Issue (required)",
  "## Knowledge Compatibility Review",
  "## Maintenance Budget Review",
  "## Testing",
];

export const dependabotAuthor = "dependabot[bot]";

export function isExemptAuthor(
  author: string | undefined,
  renovateAppSlug: string | undefined,
): boolean {
  if (author === dependabotAuthor) return true;
  if (!renovateAppSlug || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(renovateAppSlug)) {
    return false;
  }
  return author === `${renovateAppSlug}[bot]`;
}

export function sectionText(body: string, heading: string): string {
  const match = body.match(
    new RegExp(`##\\s*${heading}\\s*\\n+([\\s\\S]*?)(?:\\n##\\s|$)`, "i"),
  );
  return match?.[1]?.trim() ?? "";
}

const maintenanceClassifications = [
  "No new semantic owner, compatibility adapter, specification authority, or test surface.",
  "A maintenance surface is added or retained; justify its owner, release basis, and minimum proof.",
] as const;

/** Require a reviewable maintenance classification, not a claim that tests passed. */
export function validateMaintenanceReview(body: string): string[] {
  const review = sectionText(body, "Maintenance Budget Review");
  if (!review) return ["Maintenance Budget Review is missing or empty."];
  const checked = (review.match(/^\s*-\s*\[[xX]\]\s+.+$/gm) ?? [])
    .map((line) => line.replace(/^\s*-\s*\[[xX]\]\s+/, "").trim());
  if (
    checked.length !== 1 ||
    !maintenanceClassifications.includes(
      checked[0] as (typeof maintenanceClassifications)[number],
    )
  ) {
    return [
      "Maintenance Budget Review must select exactly one valid classification.",
    ];
  }
  if (checked[0] !== maintenanceClassifications[1]) return [];
  const errors: string[] = [];
  for (const name of ["Owner", "Release basis", "Proof"]) {
    const note = review.split("\n")
      .find((line) => line.trimStart().startsWith(name + ":"))
      ?.trimStart().slice(name.length + 1).trim() ?? "";
    if (!note || /^<.*>$/.test(note) || /^(?:todo|tbd)$/i.test(note)) {
      errors.push(
        "Maintenance Budget Review requires a concrete " + name +
          ": note for a changed surface.",
      );
    }
  }
  return errors;
}

export function validatePrBody(body: string): string[] {
  const errors: string[] = [];

  for (const section of requiredSections) {
    if (!body.toLowerCase().includes(section.toLowerCase())) {
      errors.push(`Missing required section: ${section}`);
    }
  }

  const summaryText = sectionText(body, "Summary");
  if (
    !summaryText || summaryText === "-" || /^-\s*$/.test(summaryText)
  ) {
    errors.push(
      "Summary section must be filled in and cannot remain the '-' placeholder.",
    );
  }

  const issueLinkPattern =
    /(?:^|\n)\s*(?:close:\s*#\d+|closes\s+#\d+)\s*(?:\n|$)/i;
  if (!issueLinkPattern.test(body)) {
    errors.push(
      "Related Issue must include either `close: #123` or `closes #123`.",
    );
  }

  errors.push(...validateKnowledgeCompatibilityReview(body));
  errors.push(...validateMaintenanceReview(body));

  const testingText = sectionText(body, "Testing");
  if (!/- \[(?: |x|X)\]/.test(testingText)) {
    errors.push("Testing section must include at least one checklist item.");
  }

  return errors;
}

export type PullRequestPointer = {
  body: string;
  author?: string;
};

export function extractPrNumberFromMergeGroupRef(ref: string): number | null {
  const match = ref.match(/(?:^|\/)pr-(\d+)(?:-|$)/);
  return match ? Number.parseInt(match[1], 10) : null;
}

export async function fetchPullRequestPointer(
  repo: string,
  prNumber: number,
  token: string,
  fetchImpl: typeof fetch = fetch,
): Promise<PullRequestPointer> {
  const response = await fetchImpl(
    `https://api.github.com/repos/${repo}/pulls/${prNumber}`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: "application/vnd.github+json",
      },
    },
  );
  if (!response.ok) {
    throw new Error(
      `Failed to load pull request #${prNumber}: HTTP ${response.status}`,
    );
  }
  const data = await response.json() as {
    body?: string | null;
    user?: { login?: string };
  };
  return { body: data.body ?? "", author: data.user?.login };
}

export async function resolvePullRequestPointer(
  env: {
    GITHUB_EVENT_NAME?: string;
    GITHUB_EVENT_PATH?: string;
    GITHUB_REF?: string;
    GITHUB_REPOSITORY?: string;
    GITHUB_TOKEN?: string;
  },
  fetchImpl: typeof fetch = fetch,
): Promise<PullRequestPointer> {
  if (env.GITHUB_EVENT_NAME === "merge_group") {
    const prNumber = extractPrNumberFromMergeGroupRef(env.GITHUB_REF ?? "");
    if (prNumber !== null && env.GITHUB_REPOSITORY && env.GITHUB_TOKEN) {
      return await fetchPullRequestPointer(
        env.GITHUB_REPOSITORY,
        prNumber,
        env.GITHUB_TOKEN,
        fetchImpl,
      );
    }
    return { body: "" };
  }
  if (!env.GITHUB_EVENT_PATH) {
    return { body: "" };
  }
  const payload = JSON.parse(
    await Deno.readTextFile(env.GITHUB_EVENT_PATH),
  ) as { pull_request?: { body?: string | null; user?: { login?: string } } };
  return {
    body: payload.pull_request?.body ?? "",
    author: payload.pull_request?.user?.login,
  };
}

if (import.meta.main) {
  const pointer = await resolvePullRequestPointer(Deno.env.toObject());
  if (isExemptAuthor(pointer.author, Deno.env.get("RENOVATE_APP_SLUG"))) {
    console.log(
      "Dependency update bot pull requests use generated bodies; skipping the human PR template gate.",
    );
    Deno.exit(0);
  }
  const errors = validatePrBody(pointer.body);
  if (errors.length > 0) {
    console.error(errors.join("\n"));
    Deno.exit(1);
  }
  console.log("PR body matches the required template.");
}
