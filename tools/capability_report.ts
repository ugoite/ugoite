#!/usr/bin/env -S deno run --allow-read

/**
 * Cross-surface capability projection (read-only).
 *
 * JOURNEY-KNOWLEDGE-001 seed: Space -> Form -> Entry create -> Entry edit ->
 * Search -> History -> Restore.
 *
 * This tool generates a projection from existing authorities and never acts
 * as one itself:
 * - Capability inventory: UGOITE_API_OPERATIONS (TypeScript) mirrored by
 *   SUPPORTED_OPERATIONS (Rust). Both must match exactly.
 * - Outcome semantics: Mitase Requirement / Criterion (normative).
 * - Surface usage: Frontend (*-api.ts via protocolFetch), CLI remote
 *   (http::execute with a portable operation name), CLI core (UgoiteService
 *   methods). Business meaning stays in the shared Rust boundary.
 * - Verification evidence: exact e2e test names plus exact Mitase
 *   `verifies` claims. Gaps are reported, never hidden.
 *
 * The emitted matrix is a generated projection, not an authority. Mitase
 * remains the specification validator; it does not execute Ugoite tests.
 * CI treats this report as informational: capability gaps must not fail
 * the build in 0.1.x. They become release blockers only via the C7 gate.
 */

const ROOT = new URL("../", import.meta.url);

export const JOURNEY_ID = "JOURNEY-KNOWLEDGE-001";

/** Projection states. Never collapse alias and absence into one value. */
export const CAPABILITY_STATES = [
  "verified",
  "evidence-gap",
  "surface-gap",
  "semantic-drift",
  "implemented-undiscoverable",
  "intentionally-not-required",
] as const;

export type CapabilityState = (typeof CAPABILITY_STATES)[number];

export interface E2ePointer {
  file: string;
  fragment: string;
}

export interface JourneyCapability {
  id: string;
  checkpoint: string;
  operations: string[];
  /** Informational pointer to the normative Mitase criterion, not a copy. */
  requirement: string;
  outcome: string;
  /** Set when every surface can reach the outcome but one surface hides it. */
  alias?: string;
  e2eEvidence: E2ePointer[];
}

export interface CapabilityRow {
  id: string;
  checkpoint: string;
  operations: string[];
  requirement: string;
  outcome: string;
  frontend: boolean;
  cliCore: boolean;
  cliRemote: boolean;
  sharedSemantics: boolean;
  e2eEvidenceFound: string[];
  e2eEvidenceMissing: string[];
  mitaseEvidenceFound: string[];
  state: CapabilityState;
  note: string;
}

export interface CapabilityReport {
  journey: string;
  inventorySize: number;
  manifestsMatch: boolean;
  rustOnly: string[];
  tsOnly: string[];
  capabilities: CapabilityRow[];
}

/**
 * JOURNEY-KNOWLEDGE-001 capability seed. Operations come from the portable
 * inventory; requirement strings point at Mitase criteria owned elsewhere.
 */
export const JOURNEY_CAPABILITIES: JourneyCapability[] = [
  {
    id: "space-establish",
    checkpoint: "Space",
    operations: ["space.create", "space.list", "space.get"],
    requirement: "REQ-API-001#criterion.space-lifecycle",
    outcome:
      "A durable Space exists and can be reopened with identical compatibility semantics regardless of surface.",
    e2eEvidence: [
      { file: "e2e/smoke.test.ts", fragment: "GET /spaces returns list" },
    ],
  },
  {
    id: "form-establish",
    checkpoint: "Form",
    operations: ["form.upsert", "form.list", "form.get"],
    requirement: "REQ-FORM-010#criterion.upsert-operation",
    outcome:
      "The user can establish a Form defining the schema used by a subsequent Entry, with equivalent schema semantics regardless of surface.",
    alias: "CLI reaches form.upsert via `form update`",
    e2eEvidence: [
      { file: "e2e/forms.test.ts", fragment: "Create and List Forms" },
    ],
  },
  {
    id: "entry-create",
    checkpoint: "Entry create",
    operations: ["entry.create", "entry.list", "entry.get"],
    requirement: "REQ-ENTRY-001#criterion.creation",
    outcome:
      "An equivalent Knowledge object is created and exactly one revision is appended.",
    e2eEvidence: [
      { file: "e2e/entries.test.ts", fragment: "creates a new entry" },
    ],
  },
  {
    id: "entry-edit",
    checkpoint: "Entry edit",
    operations: ["entry.update"],
    requirement: "REQ-ENTRY-003#criterion.update",
    outcome:
      "Optimistic concurrency and validation behave identically; a stale parent revision conflicts instead of overwriting.",
    e2eEvidence: [
      {
        file: "e2e/entries.test.ts",
        fragment: "consecutive PUT should succeed with updated revision_id",
      },
      {
        file: "e2e/entries.test.ts",
        fragment: "PUT with stale revision_id should return 409 conflict",
      },
    ],
  },
  {
    id: "search-keyword",
    checkpoint: "Search",
    operations: ["search.keyword"],
    requirement: "REQ-SRCH-001#criterion.keyword-search",
    outcome:
      "The updated durable Entry is discoverable under identical query conditions regardless of surface.",
    e2eEvidence: [
      {
        file: "e2e/search-ui.test.ts",
        fragment: "REQ-SRCH-004: search page starts with direct keyword search",
      },
    ],
  },
  {
    id: "entry-history",
    checkpoint: "History",
    operations: ["entry.history", "entry.revision"],
    requirement: "REQ-ENTRY-005#criterion.history",
    outcome:
      "Create and edit remain observable as append-only history without rewriting prior revisions.",
    e2eEvidence: [
      { file: "e2e/entries.test.ts", fragment: "history.revisions" },
    ],
  },
  {
    id: "entry-restore",
    checkpoint: "Restore",
    operations: ["entry.restore"],
    requirement: "REQ-ENTRY-005#criterion.history",
    outcome:
      "Restore appends a new revision or change; history never shortens and no destructive rollback occurs.",
    e2eEvidence: [
      {
        file: "e2e/knowledge-journey.test.ts",
        fragment:
          "JOURNEY-KNOWLEDGE-001: Restore appends a new revision without shortening history",
      },
    ],
  },
];

/** CLI core service methods mapped to the portable operations they fulfil. */
export const SERVICE_METHOD_OPERATIONS: Record<string, string> = {
  create_operator_space: "space.create",
  list_space_ids: "space.list",
  get_space: "space.get",
  upsert_form: "form.upsert",
  list_forms: "form.list",
  get_form: "form.get",
  list_entries: "entry.list",
  get_entry: "entry.get",
  create_entry: "entry.create",
  update_entry: "entry.update",
  delete_entry: "entry.delete",
  entry_history: "entry.history",
  entry_revision: "entry.revision",
  restore_entry: "entry.restore",
  search_entries: "search.keyword",
};

const OPERATION_PATTERN = /"([a-z][a-z0-9_.]+)"/g;

export function quotedOperations(block: string): string[] {
  return [...block.matchAll(OPERATION_PATTERN)].map((match) => match[1]);
}

export function rustManifestOperations(source: string): string[] {
  const match = source.match(
    /pub const SUPPORTED_OPERATIONS: &\[&str\] = &\[(.*?)\];/s,
  );
  if (!match) {
    throw new Error("Rust SUPPORTED_OPERATIONS manifest was not found");
  }
  return quotedOperations(match[1]);
}

export function typescriptManifestOperations(source: string): string[] {
  const match = source.match(
    /export const UGOITE_API_OPERATIONS = \[(.*?)\] as const;/s,
  );
  if (!match) {
    throw new Error("TypeScript UGOITE_API_OPERATIONS manifest was not found");
  }
  return quotedOperations(match[1]);
}

async function* walk(dir: URL): AsyncGenerator<string> {
  for await (const entry of Deno.readDir(dir)) {
    // Recover a child URL from the directory URL and entry name.
    const child = new URL(entry.name, `${dir.href.replace(/\/$/, "")}/`);
    if (entry.isDirectory) {
      if (["node_modules", "target", ".output", "dist"].includes(entry.name)) {
        continue;
      }
      yield* walk(child);
      continue;
    }
    yield decodeURIComponent(child.pathname);
  }
}

async function readUnderRoot(relativePath: string): Promise<string> {
  return await Deno.readTextFile(new URL(relativePath, ROOT));
}

async function collectFiles(
  relativeDir: string,
  suffix: string,
): Promise<string[]> {
  const files: string[] = [];
  for await (const path of walk(new URL(relativeDir, ROOT))) {
    if (path.endsWith(suffix)) files.push(path);
  }
  return files.sort();
}

function repoRelative(pathname: string): string {
  const rootPath = decodeURIComponent(new URL(ROOT).pathname).replace(
    /\/$/,
    "",
  );
  return pathname.startsWith(`${rootPath}/`)
    ? pathname.slice(rootPath.length + 1)
    : pathname;
}

/**
 * Classify one capability. Priority is deliberate and documented:
 * semantic-drift > surface-gap > implemented-undiscoverable >
 * evidence-gap > verified. An aliased-but-present surface (CLI `form
 * update` fulfilling form.upsert) must read as a discoverability finding,
 * never as a missing capability.
 */
export function classify(
  operationsPresent: {
    frontend: boolean;
    cliCore: boolean;
    cliRemote: boolean;
  },
  sharedSemantics: boolean,
  alias: string | undefined,
  e2eMissing: string[],
  mitaseFound: string[],
): { state: CapabilityState; note: string } {
  if (!sharedSemantics) {
    return {
      state: "semantic-drift",
      note:
        "Operation inventory or shared Rust encoding disagrees across adapters.",
    };
  }
  const missing = (Object.entries(operationsPresent) as Array<
    [keyof typeof operationsPresent, boolean]
  >).filter(([, present]) => !present).map(([surface]) => surface);
  if (missing.length > 0) {
    return {
      state: "surface-gap",
      note: `Missing on: ${missing.join(", ")}.`,
    };
  }
  if (alias) {
    const evidence = mitaseFound.length === 0
      ? " Mitase verification claim is still missing."
      : "";
    return {
      state: "implemented-undiscoverable",
      note: `${alias}.${evidence}`,
    };
  }
  if (e2eMissing.length > 0 || mitaseFound.length === 0) {
    const parts: string[] = [];
    if (e2eMissing.length > 0) {
      parts.push(`e2e evidence missing: ${e2eMissing.join("; ")}`);
    }
    if (mitaseFound.length === 0) {
      parts.push("Mitase verification claim missing");
    }
    return { state: "evidence-gap", note: `${parts.join(". ")}.` };
  }
  return {
    state: "verified",
    note: "All surfaces reach the outcome with exact evidence attached.",
  };
}

export async function buildReport(): Promise<CapabilityReport> {
  const rustSource = await readUnderRoot("crates/ugoite-api-client/src/lib.rs");
  const tsSource = await readUnderRoot(
    "frontend/src/lib/ugoite-client/protocol.ts",
  );
  const rustOperations = rustManifestOperations(rustSource);
  const tsOperations = typescriptManifestOperations(tsSource);
  const rustSet = new Set(rustOperations);
  const tsSet = new Set(tsOperations);
  const rustOnly = [...rustSet].filter((op) => !tsSet.has(op)).sort();
  const tsOnly = [...tsSet].filter((op) => !rustSet.has(op)).sort();
  const manifestsMatch = rustOnly.length === 0 && tsOnly.length === 0 &&
    rustOperations.length === tsOperations.length &&
    rustOperations.every((op, index) => op === tsOperations[index]);

  const prepareSection = sectionSlice(
    rustSource,
    "pub fn prepare_request",
    "pub fn decode_response",
  );
  const specSection = sectionSlice(
    rustSource,
    "fn operation_spec",
    "fn required_string",
  );

  // Frontend: quoted portable operations referenced under frontend/src/lib.
  const frontendOps = new Set<string>();
  for (const path of await collectFiles("frontend/src/lib/", ".ts")) {
    if (path.endsWith(".test.ts") || path.endsWith(".test.tsx")) continue;
    if (path.includes("/generated/")) continue;
    const content = await Deno.readTextFile(path);
    for (const op of quotedOperations(content)) {
      if (rustSet.has(op)) frontendOps.add(op);
    }
  }

  // CLI: remote via http::execute (compat path) or the target-aware
  // http::execute[_bytes|_multipart][_for_target] boundary, core via UgoiteService methods.
  const cliRemoteOps = new Set<string>();
  const cliCoreOps = new Set<string>();
  for (const path of await collectFiles("crates/ugoite-cli/src/", ".rs")) {
    const content = await Deno.readTextFile(path);
    for (
      const match of content.matchAll(
        /http::execute(?:_bytes|_multipart)?(?:_for_target)?\s*\(/g,
      )
    ) {
      const window = content.slice(match.index ?? 0, (match.index ?? 0) + 300);
      const operation = quotedOperations(window).find((candidate) =>
        candidate.includes(".")
      );
      if (operation && rustSet.has(operation)) cliRemoteOps.add(operation);
    }
    for (const match of content.matchAll(/service\s*\.\s*([a-z_]+)/g)) {
      const operation = SERVICE_METHOD_OPERATIONS[match[1]];
      if (operation) cliCoreOps.add(operation);
    }
  }

  // Mitase: exact `criterion: REQ-...#criterion....` verification claims.
  const mitaseClaims = new Map<string, string[]>();
  for (const dir of ["docs/mitase/requirements/", "docs/mitase/features/"]) {
    for (const path of await collectFiles(dir, ".yaml")) {
      const content = await Deno.readTextFile(path);
      for (
        const match of content.matchAll(
          /criterion:\s*(REQ-[A-Z0-9-]+#criterion\.[a-z0-9-]+)/g,
        )
      ) {
        const key = match[1];
        const rel = repoRelative(path);
        mitaseClaims.set(key, [...(mitaseClaims.get(key) ?? []), rel]);
      }
    }
  }

  const capabilities: CapabilityRow[] = [];
  for (const capability of JOURNEY_CAPABILITIES) {
    const sharedSemantics = manifestsMatch &&
      capability.operations.every((op) =>
        prepareSection.includes(`"${op}" =>`) &&
        specSection.includes(`"${op}" =>`)
      );
    const frontend = capability.operations.every((op) => frontendOps.has(op));
    const cliCore = capability.operations.every((op) => cliCoreOps.has(op));
    const cliRemote = capability.operations.every((op) => cliRemoteOps.has(op));

    const e2eEvidenceFound: string[] = [];
    const e2eEvidenceMissing: string[] = [];
    for (const pointer of capability.e2eEvidence) {
      let content: string;
      try {
        content = await readUnderRoot(pointer.file);
      } catch {
        e2eEvidenceMissing.push(`${pointer.file} is unreadable`);
        continue;
      }
      if (content.includes(pointer.fragment)) {
        e2eEvidenceFound.push(`${pointer.file} :: ${pointer.fragment}`);
      } else {
        e2eEvidenceMissing.push(`${pointer.file} :: ${pointer.fragment}`);
      }
    }
    if (capability.e2eEvidence.length === 0) {
      e2eEvidenceMissing.push("no e2e pointer wired for this capability yet");
    }
    const mitaseEvidenceFound = (mitaseClaims.get(capability.requirement) ?? [])
      .map((file) => `${file} verifies ${capability.requirement}`);

    const { state, note } = classify(
      { frontend, cliCore, cliRemote },
      sharedSemantics,
      capability.alias,
      e2eEvidenceMissing,
      mitaseEvidenceFound,
    );
    capabilities.push({
      id: capability.id,
      checkpoint: capability.checkpoint,
      operations: capability.operations,
      requirement: capability.requirement,
      outcome: capability.outcome,
      frontend,
      cliCore,
      cliRemote,
      sharedSemantics,
      e2eEvidenceFound,
      e2eEvidenceMissing,
      mitaseEvidenceFound,
      state,
      note,
    });
  }

  return {
    journey: JOURNEY_ID,
    inventorySize: rustOperations.length,
    manifestsMatch,
    rustOnly,
    tsOnly,
    capabilities,
  };
}

function sectionSlice(source: string, start: string, end: string): string {
  const startIndex = source.indexOf(start);
  if (startIndex === -1) return "";
  const rest = source.slice(startIndex + start.length);
  const endIndex = rest.indexOf(end);
  return endIndex === -1 ? rest : rest.slice(0, endIndex);
}

export function renderMarkdown(report: CapabilityReport): string {
  const lines: string[] = [
    `# Cross-surface capability projection (${report.journey})`,
    "",
    "Generated projection. Not an authority: outcome semantics stay with",
    "Mitase Requirement / Criterion, inventory with UGOITE_API_OPERATIONS.",
    "",
    `- Inventory: ${report.inventorySize} operations, manifests match: ${
      report.manifestsMatch ? "yes" : "NO"
    }`,
  ];
  if (!report.manifestsMatch) {
    lines.push(`- Rust-only: ${report.rustOnly.join(", ") || "-"}`);
    lines.push(`- TypeScript-only: ${report.tsOnly.join(", ") || "-"}`);
  }
  lines.push(
    "",
    "| Capability | Frontend | CLI core | CLI remote | Shared semantics | Verification | State |",
    "| --- | --- | --- | --- | --- | --- | --- |",
  );
  for (const row of report.capabilities) {
    const check = (value: boolean): string => value ? "yes" : "no";
    const verification = row.e2eEvidenceFound.length > 0 ||
        row.mitaseEvidenceFound.length > 0
      ? "partial"
      : "missing";
    lines.push(
      `| ${row.id} | ${check(row.frontend)} | ${check(row.cliCore)} | ${
        check(row.cliRemote)
      } | ${check(row.sharedSemantics)} | ${verification} | ${row.state} |`,
    );
  }
  lines.push("", "## Notes", "");
  for (const row of report.capabilities) {
    lines.push(`- ${row.id} (${row.state}): ${row.note}`);
  }
  lines.push("");
  return lines.join("\n");
}

if (import.meta.main) {
  const args = new Set(Deno.args);
  try {
    const report = await buildReport();
    if (args.has("--json")) {
      console.log(JSON.stringify(report, null, 2));
    } else {
      console.log(renderMarkdown(report));
    }
  } catch (error) {
    console.error(
      `capability projection failed: ${
        error instanceof Error ? error.message : String(error)
      }`,
    );
    Deno.exit(1);
  }
}
