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

/** Projection states describe reachability and declared evidence. */
export const CAPABILITY_STATES = [
  "verified",
  "evidence-gap",
  "surface-gap",
  "semantic-drift",
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

export const PREFLIGHT_SCOPE = "v0.3-preflight" as const;
export const PREFLIGHT_AVAILABILITY = [
  "existing",
  "existing-needs-surface-work",
  "not-implemented",
  "intentionally-not-required",
  "unresolved",
] as const;
export const PREFLIGHT_EVIDENCE_STATUS = [
  "declared",
  "source-located",
  "executed-and-passed",
  "missing",
  "not-run",
] as const;

export type PreflightAvailability = (typeof PREFLIGHT_AVAILABILITY)[number];
export type PreflightEvidenceStatus =
  (typeof PREFLIGHT_EVIDENCE_STATUS)[number];

export interface PreflightSurfaceObservation {
  surface: string;
  expected: "required" | "not-required";
  observed: boolean | null;
  evidence: string[];
  reason?: string;
}

export interface PreflightRowSeed {
  id: string;
  label: string;
  operations?: string[];
  host_resource?: string[];
  requirement_criterion_refs: string[];
  feature_binding_refs: string[];
  artifact_paths: string[];
  surface_expectations: Array<{
    surface: string;
    expected: "required" | "not-required";
    evidence_paths: string[];
    reason?: string;
  }>;
  verification_claim_refs: string[];
  test_selectors: Array<{ path: string; selector: string }>;
  availability: PreflightAvailability;
  reason: string;
  follow_up_issue: string | null;
}

export interface PreflightRow extends PreflightRowSeed {
  scope: typeof PREFLIGHT_SCOPE;
  surface_observations: PreflightSurfaceObservation[];
  evidence_status: PreflightEvidenceStatus;
}

export interface PreflightReport {
  scope: typeof PREFLIGHT_SCOPE;
  generated_projection: true;
  authority: "not-authority";
  evidence_limit: "static-evidence-not-executed-proof";
  inventorySize: number;
  manifestsMatch: boolean;
  rustOnly: string[];
  tsOnly: string[];
  rows: PreflightRow[];
}

/**
 * PRE-01 inventory. These references are pointers only: normative outcomes
 * remain in Mitase and implemented behavior remains in the named artifacts.
 */
export const PREFLIGHT_ROWS: PreflightRowSeed[] = [
  {
    id: "entry-query-page-count",
    label: "EntryQuery page / count",
    operations: ["entry.query", "entry.query.count"],
    requirement_criterion_refs: [
      "REQ-SRCH-006#criterion.query-operation",
      "REQ-FE-020#criterion.entry-query-filtering",
    ],
    feature_binding_refs: [
      "FEAT-JOURNEY-001#binding.frontend/target.entry-query",
    ],
    artifact_paths: [
      "frontend/src/lib/entry-query.ts",
      "crates/ugoite-core/src/entry_query.rs",
      "crates/ugoite-api-client/src/lib.rs",
      "crates/ugoite-cli/src/commands/entry_query.rs",
      "crates/ugoite-cli/src/http.rs",
      "crates/ugoite-server/src/lib.rs",
    ],
    surface_expectations: [
      {
        surface: "Browser",
        expected: "required",
        evidence_paths: ["frontend/src/lib/entry-query.ts"],
      },
      {
        surface: "CLI core",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/commands/entry_query.rs",
          "crates/ugoite-core/src/entry_query.rs",
        ],
      },
      {
        surface: "CLI remote",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/commands/entry_query.rs",
          "crates/ugoite-cli/src/http.rs",
        ],
      },
    ],
    verification_claim_refs: [
      "docs/mitase/requirements/search.yaml#REQ-SRCH-006/binding.entry-query-verification/page-case",
    ],
    test_selectors: [
      {
        path: "crates/ugoite-server/src/lib.rs",
        selector:
          "canonical_entry_query_returns_projected_rows_and_separate_count",
      },
    ],
    availability: "existing-needs-surface-work",
    reason:
      "Page and count are separate operations. Browser request cancellation and generation-safe retry are not established by static reachability.",
    follow_up_issue: "#3120",
  },
  {
    id: "stateless-sql-page-count",
    label: "Stateless SQL page / count",
    operations: ["sql.query", "sql.query.count"],
    requirement_criterion_refs: [
      "REQ-API-015#criterion.canonical-sql-page",
      "REQ-API-015#criterion.canonical-sql-count",
      "REQ-API-015#criterion.local-remote-parity",
    ],
    feature_binding_refs: [
      "FEAT-API-001#binding.frontend/target.sql-api",
    ],
    artifact_paths: [
      "crates/ugoite-api-client/src/lib.rs",
      "crates/ugoite-core/src/sql_query.rs",
      "frontend/src/lib/sql-api.ts",
      "crates/ugoite-cli/src/commands/sql.rs",
      "crates/ugoite-cli/src/http.rs",
      "crates/ugoite-cli/tests/test_cli_endpoint_routing.rs",
    ],
    surface_expectations: [
      {
        surface: "Browser",
        expected: "required",
        evidence_paths: ["frontend/src/lib/sql-api.ts"],
      },
      {
        surface: "CLI core",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/commands/sql.rs",
          "crates/ugoite-core/src/sql_query.rs",
        ],
      },
      {
        surface: "CLI remote",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/commands/sql.rs",
          "crates/ugoite-cli/src/http.rs",
        ],
      },
    ],
    verification_claim_refs: [
      "docs/mitase/requirements/api.yaml#REQ-API-015/binding.cli-verification/remote-page",
      "docs/mitase/requirements/api.yaml#REQ-API-015/binding.cli-verification/remote-count",
    ],
    test_selectors: [
      {
        path: "crates/ugoite-cli/tests/test_cli_endpoint_routing.rs",
        selector: "test_sql_query_uses_stateless_route_and_dto",
      },
      {
        path: "crates/ugoite-cli/tests/test_cli_endpoint_routing.rs",
        selector: "test_sql_query_count_uses_separate_stateless_route",
      },
    ],
    availability: "existing-needs-surface-work",
    reason:
      "Read-only limits, ORDER BY, continuation and explicit count are separate semantics. Multi-page CLI export is not implemented.",
    follow_up_issue: "#3130",
  },
  {
    id: "form-discovery-get-save",
    label: "Form discovery / get / save",
    operations: ["form.list", "form.get", "form.upsert"],
    host_resource: ["ugoite://form/{id}"],
    requirement_criterion_refs: [
      "REQ-FORM-010#criterion.list-operation",
      "REQ-FORM-010#criterion.get-operation",
      "REQ-FORM-010#criterion.upsert-operation",
      "REQ-JOURNEY-001#criterion.form-establish",
    ],
    feature_binding_refs: [
      "FEAT-JOURNEY-001#binding.frontend/target.form-api",
      "FEAT-JOURNEY-001#binding.cli-core/target.form-command",
    ],
    artifact_paths: [
      "frontend/src/lib/form-api.ts",
      "crates/ugoite-cli/src/commands/form.rs",
      "crates/ugoite-core/src/lib.rs",
      "crates/ugoite-cli/src/http.rs",
      "crates/ugoite-server/src/mcp.rs",
      "e2e/forms.test.ts",
    ],
    surface_expectations: [
      {
        surface: "Browser",
        expected: "required",
        evidence_paths: ["frontend/src/lib/form-api.ts"],
      },
      {
        surface: "CLI core",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/commands/form.rs",
          "crates/ugoite-core/src/lib.rs",
        ],
      },
      {
        surface: "CLI remote",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/commands/form.rs",
          "crates/ugoite-cli/src/http.rs",
        ],
      },
      {
        surface: "MCP resource read",
        expected: "not-required",
        evidence_paths: ["crates/ugoite-server/src/mcp.rs"],
        reason:
          "The MCP Form resource is a read-only facade; it does not perform REST/CLI Form upsert.",
      },
    ],
    verification_claim_refs: [
      "docs/mitase/requirements/journey.yaml#REQ-JOURNEY-001/binding.journey-frontend-verification/form-case",
    ],
    test_selectors: [
      { path: "e2e/forms.test.ts", selector: "Create and List Forms" },
    ],
    availability: "existing",
    reason:
      "MCP resource reads and portable Form mutations are distinct capabilities.",
    follow_up_issue: null,
  },
  {
    id: "knowledge-mutation-recovery",
    label: "Knowledge mutation / restore",
    operations: [
      "entry.create",
      "entry.update",
      "entry.delete",
      "entry.restore",
      "run.undo",
    ],
    requirement_criterion_refs: [
      "REQ-JOURNEY-001#criterion.entry-create",
      "REQ-JOURNEY-001#criterion.entry-edit",
      "REQ-JOURNEY-001#criterion.entry-restore",
      "REQ-API-002#criterion.entry-lifecycle",
    ],
    feature_binding_refs: [
      "FEAT-JOURNEY-001#binding.frontend/target.entry-api",
      "FEAT-API-001#binding.cli-implementation/target.entry-command",
    ],
    artifact_paths: [
      "crates/ugoite-core/src/lib.rs",
      "crates/ugoite-api-client/src/lib.rs",
      "crates/ugoite-cli/src/commands/entry.rs",
      "crates/ugoite-cli/src/http.rs",
      "crates/ugoite-server/src/mcp.rs",
      "frontend/src/lib/entry-api.ts",
      "e2e/knowledge-journey.test.ts",
    ],
    surface_expectations: [
      {
        surface: "Browser",
        expected: "required",
        evidence_paths: ["frontend/src/lib/entry-api.ts"],
      },
      {
        surface: "CLI core",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/commands/entry.rs",
          "crates/ugoite-core/src/lib.rs",
        ],
      },
      {
        surface: "CLI remote",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/commands/entry.rs",
          "crates/ugoite-cli/src/http.rs",
        ],
      },
      {
        surface: "MCP semantic facade",
        expected: "required",
        evidence_paths: ["crates/ugoite-server/src/mcp.rs"],
      },
      { surface: "Konase Host", expected: "required", evidence_paths: [] },
    ],
    verification_claim_refs: [
      "docs/mitase/requirements/journey.yaml#REQ-JOURNEY-001/binding.journey-frontend-verification/entry-create-case",
      "docs/mitase/requirements/journey.yaml#REQ-JOURNEY-001/binding.journey-frontend-verification/restore-case",
    ],
    test_selectors: [
      {
        path: "e2e/knowledge-journey.test.ts",
        selector:
          "JOURNEY-KNOWLEDGE-001: Entry create appends exactly one revision",
      },
      {
        path: "e2e/knowledge-journey.test.ts",
        selector:
          "JOURNEY-KNOWLEDGE-001: Restore appends a new revision without shortening history",
      },
    ],
    availability: "unresolved",
    reason:
      "Receipt, ACL, conflict, append-only Restore and Undo outcomes must be evaluated by each Host. MCP save/delete are not portable REST operations.",
    follow_up_issue: "#3124",
  },
  {
    id: "konase-host-confirmation",
    label: "Konase Host / confirmation",
    host_resource: ["AskConfirmation", "ugoite.save", "ugoite.delete"],
    requirement_criterion_refs: [
      "REQ-JOURNEY-001#criterion.entry-create",
      "REQ-API-002#criterion.entry-lifecycle",
    ],
    feature_binding_refs: [],
    artifact_paths: [
      "crates/ugoite-konase/src/lib.rs",
      "crates/ugoite-cli/src/commands/konase.rs",
      "crates/ugoite-cli/src/http.rs",
      "docs/architecture/boundaries/konase.md",
    ],
    surface_expectations: [
      {
        surface: "CLI Host",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/commands/konase.rs",
          "crates/ugoite-konase/src/lib.rs",
        ],
      },
      { surface: "Browser Host", expected: "required", evidence_paths: [] },
    ],
    verification_claim_refs: [],
    test_selectors: [],
    availability: "existing-needs-surface-work",
    reason:
      "Engine confirmation types do not establish a working Host approval UX. CLI work is tracked separately; Browser Host remains follow-up.",
    follow_up_issue: "#3124",
  },
  {
    id: "portable-frontend-protocol",
    label: "Portable frontend protocol",
    operations: [],
    requirement_criterion_refs: [
      "REQ-API-015#criterion.local-remote-parity",
    ],
    feature_binding_refs: [
      "FEAT-API-001#binding.frontend/target.sql-api",
    ],
    artifact_paths: [
      "frontend/src/lib/ugoite-client/protocol.ts",
      "crates/ugoite-api-client/src/lib.rs",
      "crates/ugoite-wasm/src/lib.rs",
      "crates/ugoite-cli/src/http.rs",
      "crates/ugoite-server/src/openapi.json",
    ],
    surface_expectations: [
      {
        surface: "Browser",
        expected: "required",
        evidence_paths: ["frontend/src/lib/ugoite-client/protocol.ts"],
      },
      {
        surface: "WASM",
        expected: "required",
        evidence_paths: ["crates/ugoite-wasm/src/lib.rs"],
      },
      {
        surface: "CLI remote",
        expected: "required",
        evidence_paths: [
          "crates/ugoite-cli/src/http.rs",
          "crates/ugoite-api-client/src/lib.rs",
        ],
      },
    ],
    verification_claim_refs: [],
    test_selectors: [
      {
        path: "tools/capability_report_test.ts",
        selector:
          "capability projection tracks the portable operation manifests",
      },
    ],
    availability: "existing",
    reason:
      "Manifest equality is a static registry fact. It does not prove per-operation reachability, E2E execution, or authorization effectiveness.",
    follow_up_issue: null,
  },
];

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
    requirement: "REQ-JOURNEY-001#criterion.form-establish",
    outcome:
      "The user can establish a Form defining the schema used by a subsequent Entry, with equivalent schema semantics regardless of surface.",
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
    operations: ["entry.query"],
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
  query_entry_page: "entry.query",
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
 * semantic-drift > surface-gap > evidence-gap > verified.
 */
export function classify(
  operationsPresent: {
    frontend: boolean;
    cliCore: boolean;
    cliRemote: boolean;
  },
  sharedSemantics: boolean,
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

function yamlItemBlock(
  source: string,
  id: string,
  indent: number,
): string | undefined {
  const lines = source.split("\n");
  const prefix = `${" ".repeat(indent)}- id: `;
  const start = lines.findIndex((line) => line === `${prefix}${id}`);
  if (start < 0) return undefined;
  let end = lines.length;
  for (let index = start + 1; index < lines.length; index++) {
    if (lines[index].startsWith(prefix)) {
      end = index;
      break;
    }
  }
  return lines.slice(start, end).join("\n");
}

export function resolveRequirementCriterionRef(
  ref: string,
  sources: Map<string, string>,
): boolean {
  const match = ref.match(/^(REQ-[A-Z0-9-]+)#criterion\.([a-z0-9-]+)$/);
  if (!match) return false;
  for (const [path, source] of sources) {
    if (!path.startsWith("docs/mitase/requirements/")) continue;
    const requirement = yamlItemBlock(source, match[1], 2);
    if (requirement && yamlItemBlock(requirement, match[2], 6)) return true;
  }
  return false;
}

export function resolveFeatureBindingRef(
  ref: string,
  sources: Map<string, string>,
): boolean {
  const match = ref.match(
    /^(FEAT-[A-Z0-9-]+)#binding\.([a-z0-9-]+)\/target\.([a-z0-9-]+)$/,
  );
  if (!match) return false;
  for (const [path, source] of sources) {
    if (!path.startsWith("docs/mitase/features/")) continue;
    const feature = yamlItemBlock(source, match[1], 2);
    const binding = feature && yamlItemBlock(feature, match[2], 6);
    if (binding && yamlItemBlock(binding, match[3], 10)) return true;
  }
  return false;
}

export function resolveVerificationClaimRef(
  ref: string,
  sources: Map<string, string>,
  expectedCriteria?: string[],
): boolean {
  const [path, fragment] = ref.split("#", 2);
  const match = fragment?.match(
    /^(REQ-[A-Z0-9-]+)\/binding\.([a-z0-9-]+)\/([a-z0-9-]+)$/,
  );
  const source = path ? sources.get(path) : undefined;
  if (!source || !match || !path.startsWith("docs/mitase/requirements/")) {
    return false;
  }
  const requirement = yamlItemBlock(source, match[1], 2);
  const binding = requirement && yamlItemBlock(requirement, match[2], 6);
  const target = binding && yamlItemBlock(binding, match[3], 10);
  return (target?.includes("kind: verifies") ?? false) &&
    (!expectedCriteria ||
      expectedCriteria.some((criterion) =>
        target?.includes(`criterion: ${criterion}`)
      ));
}

export function validatePreflightSeedShape(
  seed: PreflightRowSeed,
  knownOperations: readonly string[],
): string[] {
  const errors: string[] = [];
  const operations = seed.operations ?? [];
  if (new Set(operations).size !== operations.length) {
    errors.push("duplicate operation in row");
  }
  for (const operation of operations) {
    if (!knownOperations.includes(operation)) {
      errors.push(`unknown operation: ${operation}`);
    }
  }
  if (
    seed.surface_expectations.some((item) =>
      item.expected === "not-required" &&
      (!item.reason?.trim() || item.evidence_paths.length === 0)
    )
  ) {
    errors.push("not-required surface lacks evidence or a reason");
  }
  return errors;
}

export function selectorLocated(source: string, selector: string): boolean {
  return selector.trim().length > 0 && source.includes(selector);
}

export async function buildPreflightReport(): Promise<PreflightReport> {
  const rustSource = await readUnderRoot("crates/ugoite-api-client/src/lib.rs");
  const tsSource = await readUnderRoot(
    "frontend/src/lib/ugoite-client/protocol.ts",
  );
  const rustOperations = rustManifestOperations(rustSource);
  const tsOperations = typescriptManifestOperations(tsSource);
  const rustSet = new Set(rustOperations);
  const tsSet = new Set(tsOperations);
  const rustOnly = [...new Set(rustSet)].filter((op) => !tsSet.has(op)).sort();
  const tsOnly = [...new Set(tsSet)].filter((op) => !rustSet.has(op)).sort();
  const manifestsMatch = rustOnly.length === 0 && tsOnly.length === 0 &&
    rustOperations.length === tsOperations.length &&
    rustOperations.every((op, index) => op === tsOperations[index]);
  const mitaseFiles = [
    ...await collectFiles("docs/mitase/requirements/", ".yaml"),
    ...await collectFiles("docs/mitase/features/", ".yaml"),
  ];
  const mitaseSources = new Map<string, string>();
  for (const path of mitaseFiles) {
    mitaseSources.set(repoRelative(path), await Deno.readTextFile(path));
  }
  const rows: PreflightRow[] = [];

  for (const seed of PREFLIGHT_ROWS) {
    const rowErrors: string[] = [];
    const operationNames = seed.id === "portable-frontend-protocol"
      ? rustOperations
      : seed.operations ?? [];
    rowErrors.push(...validatePreflightSeedShape(seed, rustOperations));
    if (
      seed.requirement_criterion_refs.some((ref) =>
        !resolveRequirementCriterionRef(ref, mitaseSources)
      )
    ) rowErrors.push("unresolved Requirement/Criterion reference");
    if (
      seed.feature_binding_refs.some((ref) =>
        !resolveFeatureBindingRef(ref, mitaseSources)
      )
    ) rowErrors.push("unresolved Feature/Artifact Binding reference");
    if (
      seed.verification_claim_refs.some((ref) =>
        !resolveVerificationClaimRef(
          ref,
          mitaseSources,
          seed.requirement_criterion_refs,
        )
      )
    ) rowErrors.push("unresolved Verification Claim reference");

    const artifactFound: string[] = [];
    const artifactMissing: string[] = [];
    for (const path of seed.artifact_paths) {
      try {
        await readUnderRoot(path);
        artifactFound.push(path);
      } catch {
        artifactMissing.push(path);
      }
    }
    const selectorFound: string[] = [];
    const selectorMissing: string[] = [];
    for (const { path, selector } of seed.test_selectors) {
      try {
        const source = await readUnderRoot(path);
        (selectorLocated(source, selector) ? selectorFound : selectorMissing)
          .push(
            `${path} :: ${selector}`,
          );
      } catch {
        selectorMissing.push(`${path} :: ${selector}`);
      }
    }
    const claimFound = seed.verification_claim_refs.filter((ref) =>
      resolveVerificationClaimRef(
        ref,
        mitaseSources,
        seed.requirement_criterion_refs,
      )
    );
    const hasDeclaredEvidence = claimFound.length > 0 ||
      seed.test_selectors.length > 0;
    const hasSourceEvidence = artifactFound.length > 0 &&
      rowErrors.length === 0;
    const evidence_status: PreflightEvidenceStatus = rowErrors.length > 0 ||
        artifactMissing.length > 0 || selectorMissing.length > 0
      ? "missing"
      : !hasDeclaredEvidence
      ? "not-run"
      : hasSourceEvidence
      ? "source-located"
      : "declared";
    const surface_observations: PreflightSurfaceObservation[] = seed
      .surface_expectations.map((expectation) => {
        const evidence = expectation.evidence_paths.filter((path) =>
          artifactFound.includes(path)
        );
        return {
          surface: expectation.surface,
          expected: expectation.expected,
          observed: expectation.evidence_paths.length > 0 &&
              evidence.length === expectation.evidence_paths.length
            ? true
            : null,
          evidence,
          ...(expectation.reason ? { reason: expectation.reason } : {}),
        };
      });
    const hasMissingRequiredSurface = surface_observations.some((item) =>
      item.expected === "required" && item.observed === null
    );
    if (hasMissingRequiredSurface && seed.availability === "existing") {
      rowErrors.push("required surface source not located");
    }
    const resultAvailability = rowErrors.length > 0
      ? "unresolved"
      : seed.availability;
    rows.push({
      ...seed,
      operations: operationNames,
      scope: PREFLIGHT_SCOPE,
      surface_observations,
      availability: resultAvailability,
      reason: rowErrors.length > 0
        ? `${seed.reason} Projection issues: ${rowErrors.join("; ")}.`
        : seed.reason,
      evidence_status,
    });
  }

  return {
    scope: PREFLIGHT_SCOPE,
    generated_projection: true,
    authority: "not-authority",
    evidence_limit: "static-evidence-not-executed-proof",
    inventorySize: rustOperations.length,
    manifestsMatch,
    rustOnly,
    tsOnly,
    rows,
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

export function renderPreflightMarkdown(report: PreflightReport): string {
  const lines = [
    `# Capability preflight projection (${report.scope})`,
    "",
    "Generated projection — not authority; static evidence, not executed proof.",
    "Outcome semantics remain in Mitase Requirements and Criteria.",
    "",
    `- Portable operation inventory: ${report.inventorySize}; TS/Rust manifests match: ${
      report.manifestsMatch ? "yes" : "NO"
    }`,
    "",
    "| ID | Capability | Operations / resource | Availability | Evidence | Follow-up |",
    "| --- | --- | --- | --- | --- | --- |",
  ];
  for (const row of report.rows) {
    lines.push(
      `| ${row.id} | ${row.label} | ${
        (row.operations ?? []).length === report.inventorySize
          ? `${report.inventorySize} registered operations`
          : (row.operations ?? []).join(", ") ||
            (row.host_resource ?? []).join(", ")
      } | ${row.availability} | ${row.evidence_status} | ${
        row.follow_up_issue ?? "—"
      } |`,
    );
  }
  lines.push("", "## Details", "");
  for (const row of report.rows) {
    lines.push(`### ${row.id}`, "", row.reason, "");
    lines.push(
      `- Criteria: ${row.requirement_criterion_refs.join(", ") || "missing"}`,
    );
    lines.push(
      `- Feature bindings: ${row.feature_binding_refs.join(", ") || "missing"}`,
    );
    lines.push(`- Artifacts: ${row.artifact_paths.join(", ") || "missing"}`);
    lines.push(
      `- Verification claims: ${
        row.verification_claim_refs.join(", ") || "missing"
      }`,
    );
    lines.push(
      `- Test selectors: ${
        row.test_selectors.map((item) => `${item.path} :: ${item.selector}`)
          .join(", ") || "not wired"
      }`,
    );
    for (const observation of row.surface_observations) {
      lines.push(
        `- ${observation.surface} (${observation.expected}): ${
          observation.observed === null ? "unresolved" : "source located"
        }${observation.reason ? ` — ${observation.reason}` : ""}`,
      );
    }
    if (row.follow_up_issue) lines.push(`- Follow-up: ${row.follow_up_issue}`);
    lines.push("");
  }
  return lines.join("\n");
}

if (import.meta.main) {
  const args = new Set(Deno.args);
  try {
    const scopeIndex = Deno.args.indexOf("--scope");
    const scope = scopeIndex >= 0 ? Deno.args[scopeIndex + 1] : undefined;
    if (scope && scope !== PREFLIGHT_SCOPE) {
      throw new Error(`unknown scope: ${scope}`);
    }
    const preflight = scope === PREFLIGHT_SCOPE;
    const report = preflight
      ? await buildPreflightReport()
      : await buildReport();
    if (args.has("--json")) {
      console.log(JSON.stringify(report, null, 2));
    } else {
      console.log(
        preflight
          ? renderPreflightMarkdown(report as PreflightReport)
          : renderMarkdown(report as CapabilityReport),
      );
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
