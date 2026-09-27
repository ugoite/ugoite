export type ImpactCategories = {
  docs: boolean;
  frontend: boolean;
  rust: boolean;
  global: boolean;
};

export type ImpactReport = {
  schemaVersion: 1;
  event: string;
  baseSha: string | null;
  headSha: string | null;
  fileCount: number | null;
  categories: ImpactCategories;
  scope: "all" | "scoped";
  reason: string;
  candidateLanes: string[];
  executionMode: "selective-pr" | "all-lanes";
  jobsSkipped: boolean;
};

const ALL_LANES = [
  "rust-check",
  "rust-test",
  "web",
  "artifacts",
  "docsite-nav",
] as const;

const ZERO_SHA = /^0{40}$/;
const SHA = /^[0-9a-f]{40}$/i;
const MAX_DIFF_FILES = 100;

export function classifyPaths(paths: string[]): ImpactCategories {
  const categories: ImpactCategories = {
    docs: false,
    frontend: false,
    rust: false,
    global: false,
  };

  for (const path of paths) {
    if (
      path.startsWith(".github/") || path === "mise.toml" ||
      path === "mise.lock" || path === ".mise.toml" ||
      path === "deno.json" || path === "deno.lock" ||
      path === "rust-toolchain" || path === "rust-toolchain.toml" ||
      path.startsWith(".cargo/") || path === ".dockerignore" ||
      path === "Dockerfile" || path.startsWith("Dockerfile.") ||
      path.startsWith("scripts/") || path.startsWith("docs/architecture/") ||
      path.startsWith("docs/mitase/")
    ) {
      categories.global = true;
      continue;
    }

    if (path.startsWith("docs/") || path.startsWith("docsite/")) {
      categories.docs = true;
      continue;
    }

    if (path.startsWith("frontend/") || path.startsWith("shared/")) {
      categories.frontend = true;
      continue;
    }

    if (
      path.startsWith("crates/") || path === "Cargo.toml" ||
      path === "Cargo.lock"
    ) {
      categories.rust = true;
      continue;
    }

    // An unclassified path makes the lane plan conservative.
    categories.global = true;
  }

  return categories;
}

function allCategories(): ImpactCategories {
  return { docs: true, frontend: true, rust: true, global: true };
}

export function makeImpactReport(input: {
  event: string;
  baseSha?: string;
  headSha?: string;
  paths?: string[];
  diffError?: string;
}): ImpactReport {
  const { event, baseSha, headSha, paths, diffError } = input;
  let reason = "changes classified";
  let categories = paths ? classifyPaths(paths) : allCategories();
  const fileCount: number | null = paths?.length ?? null;
  let scope: ImpactReport["scope"] = "scoped";

  if (event === "merge_group") {
    reason = "merge-group validation always covers every lane";
    categories = allCategories();
    scope = "all";
  } else if (diffError) {
    reason = "diff unavailable; conservative all-lanes fallback";
    categories = allCategories();
    scope = "all";
  } else if (!baseSha || !headSha || !SHA.test(baseSha) || !SHA.test(headSha)) {
    reason = "base or head SHA is missing or invalid";
    categories = allCategories();
    scope = "all";
  } else if (ZERO_SHA.test(baseSha)) {
    reason = "base SHA is the all-zero sentinel";
    categories = allCategories();
    scope = "all";
  } else if (event !== "pull_request" && event !== "push") {
    reason = `unsupported event: ${event || "unknown"}`;
    categories = allCategories();
    scope = "all";
  } else if (!paths) {
    reason = "changed paths are unavailable";
    categories = allCategories();
    scope = "all";
  } else if (paths.length > MAX_DIFF_FILES) {
    reason = `diff exceeds ${MAX_DIFF_FILES} files`;
    categories = allCategories();
    scope = "all";
  } else if (categories.global) {
    reason = "global or unclassified path requires all lanes";
    scope = "all";
  } else if (!categories.docs && !categories.frontend && !categories.rust) {
    reason = "empty diff requires all lanes";
    categories = allCategories();
    scope = "all";
  }

  const candidateLanes = new Set<string>();
  if (scope !== "all" && event === "pull_request") {
    if (categories.docs) {
      candidateLanes.add("docsite-nav");
      candidateLanes.add("web");
    }
    if (categories.frontend) {
      candidateLanes.add("web");
      candidateLanes.add("artifacts");
    }
    if (categories.rust) {
      for (const lane of ["rust-check", "rust-test", "web", "artifacts"]) {
        candidateLanes.add(lane);
      }
    }
  }

  // Main pushes must retain the artifact lane for the docsite Pages payload and
  // its source manifest. Other non-PR events also stay on the full gate.
  if (event !== "pull_request" || scope === "all") {
    scope = "all";
    if (event === "push") {
      reason = "main pushes retain every lane and publish verified artifacts";
    }
    for (const lane of ALL_LANES) candidateLanes.add(lane);
  }

  const plannedLanes = [...candidateLanes];
  const jobsSkipped = plannedLanes.length < ALL_LANES.length;

  return {
    schemaVersion: 1,
    event,
    baseSha: baseSha ?? null,
    headSha: headSha ?? null,
    fileCount,
    categories,
    scope,
    reason,
    candidateLanes: plannedLanes.sort(),
    executionMode: jobsSkipped ? "selective-pr" : "all-lanes",
    jobsSkipped,
  };
}

function parseArgs(args: string[]): Record<string, string> {
  const values: Record<string, string> = {};
  for (let index = 0; index < args.length; index++) {
    const key = args[index];
    if (!key?.startsWith("--")) continue;
    const value = args[index + 1];
    if (value && !value.startsWith("--")) {
      values[key.slice(2)] = value;
      index++;
    }
  }
  return values;
}

async function changedPaths(
  baseSha: string,
  headSha: string,
): Promise<string[]> {
  const output = await new Deno.Command("git", {
    args: [
      "diff",
      "--no-ext-diff",
      "--no-renames",
      "--name-only",
      "-z",
      baseSha,
      headSha,
    ],
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (!output.success) {
    const stderr = new TextDecoder().decode(output.stderr).trim();
    throw new Error(stderr || `git diff exited with ${output.code}`);
  }
  return new TextDecoder().decode(output.stdout).split("\0").filter(Boolean);
}

async function resolveCommit(reference: string): Promise<string> {
  const output = await new Deno.Command("git", {
    args: [
      "rev-parse",
      "--verify",
      "--end-of-options",
      `${reference}^{commit}`,
    ],
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (!output.success) {
    const stderr = new TextDecoder().decode(output.stderr).trim();
    throw new Error(stderr || `git rev-parse exited with ${output.code}`);
  }
  return new TextDecoder().decode(output.stdout).trim();
}

function markdown(report: ImpactReport): string {
  const categories = Object.entries(report.categories)
    .filter(([, enabled]) => enabled)
    .map(([name]) => name)
    .join(", ") || "none";
  return [
    "### CI impact report",
    "",
    `- Proposed scope: **${report.scope}** (${report.reason})`,
    `- Categories: ${categories}`,
    `- Changed files examined: ${report.fileCount ?? "unavailable"}`,
    `- Candidate lanes: ${report.candidateLanes.join(", ")}`,
    `- Execution: **${report.executionMode}**; jobs skipped: **${
      report.jobsSkipped ? "yes" : "no"
    }**`,
  ].join("\n");
}

export async function runImpactCli(args = Deno.args): Promise<ImpactReport> {
  const flags = parseArgs(args);
  const event = Deno.env.get("CI_IMPACT_EVENT") ?? flags.event ?? "local";
  const baseSha = Deno.env.get("CI_IMPACT_BASE_SHA") ?? flags.base;
  const headSha = Deno.env.get("CI_IMPACT_HEAD_SHA") ?? flags.head;
  let report: ImpactReport;

  if (event === "merge_group") {
    report = makeImpactReport({ event });
  } else if (baseSha && headSha) {
    try {
      const resolvedBaseSha = await resolveCommit(baseSha);
      const resolvedHeadSha = await resolveCommit(headSha);
      const paths = await changedPaths(resolvedBaseSha, resolvedHeadSha);
      report = makeImpactReport({
        event,
        baseSha: resolvedBaseSha,
        headSha: resolvedHeadSha,
        paths,
      });
    } catch (error) {
      report = makeImpactReport({
        event,
        baseSha,
        headSha,
        diffError: error instanceof Error ? error.message : String(error),
      });
    }
  } else {
    report = makeImpactReport({ event, baseSha, headSha });
  }

  const json = JSON.stringify(report);
  const outputPath = Deno.env.get("GITHUB_OUTPUT");
  if (outputPath) {
    await Deno.writeTextFile(
      outputPath,
      `plan_status=ok\nplan_scope=${report.scope}\njobs_skipped=${report.jobsSkipped}\n${
        ALL_LANES.map((lane) =>
          `plan_${lane.replaceAll("-", "_")}=${
            report.candidateLanes.includes(lane)
          }`
        ).join("\n")
      }\nreport_json=${json}\n`,
      { append: true },
    );
  }
  const summaryPath = Deno.env.get("GITHUB_STEP_SUMMARY");
  if (summaryPath) {
    await Deno.writeTextFile(summaryPath, `${markdown(report)}\n`, {
      append: true,
    });
  }
  console.log(json);
  return report;
}

if (import.meta.main) await runImpactCli();
