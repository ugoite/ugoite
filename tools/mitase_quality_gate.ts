/**
 * Repository-owned Mitase quality gate (U-02).
 *
 * Usage:
 *   deno run -A tools/mitase_quality_gate.ts
 *
 * The tool performs exactly one deterministic operation:
 * 1. Runs the pinned `./scripts/mitase check . --format json` (read-only
 *    repository validation; it never executes tests or mutates the tree).
 * 2. Computes stable fingerprints for every `MITASE-QUALITY-*` diagnostic
 *    from rule code, subject, reference, and related anchors only. File line
 *    numbers and full statement text never enter a fingerprint, so moving a
 *    specification across lines or files does not count as a new finding.
 * 3. Fails closed when a Q001-Q004 fingerprint appears that is not recorded
 *    in `tools/mitase_quality_baseline.json`. Q005 (info) is recorded but
 *    never fails the gate.
 * 4. Reports resolved baseline entries so follow-up work can prune them.
 *
 * The baseline grows only through deliberate reviewed commits, each carrying
 * a design reason. This tool consumes existing diagnostics; it grants Mitase
 * no execution, planning, or workspace-mutation ability.
 */

export const BASELINE_PATH = "tools/mitase_quality_baseline.json";

// Only these severities of structural suspicion block the gate. Q005 stays
// informational by contract and is recorded for description cleanup.
const ENFORCED_CODES = new Set([
  "MITASE-QUALITY-001",
  "MITASE-QUALITY-002",
  "MITASE-QUALITY-003",
  "MITASE-QUALITY-004",
]);

const QUALITY_PREFIX = "MITASE-QUALITY-";

export type DiagnosticSubject = {
  kind: string;
  value: string;
};

export type QualityDiagnostic = {
  code: string;
  subject?: DiagnosticSubject | null;
  reference?: DiagnosticSubject | null;
  relation?: {
    relation: string;
    source: DiagnosticSubject;
    targets: DiagnosticSubject[];
  } | null;
};

export type BaselineFile = {
  mitase_version: string;
  entries: { fingerprint: string; note: string }[];
};

/** Stable identity for one quality diagnostic across moves and rewraps. */
export function qualityFingerprint(diagnostic: QualityDiagnostic): string {
  const subject = diagnostic.subject?.value ?? "";
  const reference = diagnostic.reference?.value ?? "";
  const related = (diagnostic.relation?.targets ?? [])
    .map((target) => target.value)
    .sort()
    .join(",");
  return `${diagnostic.code}|${subject}|${reference}|${related}`;
}

export function isQuality(diagnostic: { code: string }): boolean {
  return diagnostic.code.startsWith(QUALITY_PREFIX);
}

export function isEnforced(diagnostic: { code: string }): boolean {
  return ENFORCED_CODES.has(diagnostic.code);
}

export type BaselineComparison = {
  /** Enforced fingerprints present now but absent from the baseline. */
  added: string[];
  /** Enforced baseline fingerprints absent now; candidates for pruning. */
  resolved: string[];
  /** Current enforced fingerprint count (for logging, not gating). */
  current: number;
};

export function compareBaselines(
  current: QualityDiagnostic[],
  baseline: BaselineFile,
): BaselineComparison {
  const known = new Set(baseline.entries.map((entry) => entry.fingerprint));
  const present = new Set<string>();
  for (const diagnostic of current) {
    if (isQuality(diagnostic) && isEnforced(diagnostic)) {
      present.add(qualityFingerprint(diagnostic));
    }
  }
  return {
    added: [...present].filter((fingerprint) => !known.has(fingerprint)).sort(),
    resolved: [...known].filter((fingerprint) => !present.has(fingerprint))
      .filter((fingerprint) =>
        [...ENFORCED_CODES].some((code) => fingerprint.startsWith(`${code}|`))
      ).sort(),
    current: present.size,
  };
}

async function runCheck(root: string): Promise<QualityDiagnostic[]> {
  const command = new Deno.Command("./scripts/mitase", {
    args: ["check", ".", "--format", "json"],
    cwd: root,
    stdout: "piped",
    stderr: "piped",
  });
  const output = await command.output();
  if (!output.success) {
    const stderr = new TextDecoder().decode(output.stderr);
    throw new Error(`mitase check failed; refusing to gate:\n${stderr}`);
  }
  const payload = JSON.parse(new TextDecoder().decode(output.stdout));
  const diagnostics = payload.diagnostics;
  if (!Array.isArray(diagnostics)) {
    throw new Error("mitase check JSON has no diagnostics array");
  }
  return diagnostics.filter(isQuality);
}

function fail(message: string): never {
  console.error(message);
  Deno.exit(1);
}

if (import.meta.main) {
  const root = Deno.cwd();
  const baseline = JSON.parse(
    await Deno.readTextFile(`${root}/${BASELINE_PATH}`),
  ) as BaselineFile;
  const current = await runCheck(root);
  const comparison = compareBaselines(current, baseline);
  if (comparison.added.length > 0) {
    fail(
      [
        `mitase quality gate: ${comparison.added.length} new Q001-Q004 finding(s):`,
        ...comparison.added.map((fingerprint) => `  new: ${fingerprint}`),
        `Review each finding, resolve it or record a design reason, and update ${BASELINE_PATH} in a reviewed commit.`,
      ].join("\n"),
    );
  }
  console.log(
    `mitase quality gate: ${comparison.current} enforced finding(s), all recorded.`,
  );
  for (const fingerprint of comparison.resolved) {
    console.log(`resolved (prune candidate): ${fingerprint}`);
  }
}
