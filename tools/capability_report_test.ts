import { assertEquals } from "@std/assert/equals";
import {
  buildPreflightReport,
  buildReport,
  CAPABILITY_STATES,
  classify,
  JOURNEY_CAPABILITIES,
  JOURNEY_ID,
  PREFLIGHT_EVIDENCE_STATUS,
  PREFLIGHT_ROWS,
  PREFLIGHT_SCOPE,
  renderMarkdown,
  renderPreflightMarkdown,
  resolveFeatureBindingRef,
  resolveRequirementCriterionRef,
  resolveVerificationClaimRef,
  rustManifestOperations,
  selectorLocated,
  SERVICE_METHOD_OPERATIONS,
  typescriptManifestOperations,
  validatePreflightSeedShape,
} from "./capability_report.ts";

// The projection inventory must track the portable operation manifests
// exactly; it never invents operations of its own.
Deno.test("capability projection tracks the portable operation manifests", async () => {
  const rustSource = await Deno.readTextFile(
    "crates/ugoite-api-client/src/lib.rs",
  );
  const tsSource = await Deno.readTextFile(
    "frontend/src/lib/ugoite-client/protocol.ts",
  );
  const rustOperations = rustManifestOperations(rustSource);
  const tsOperations = typescriptManifestOperations(tsSource);
  assertEquals(rustOperations.length, 65);
  assertEquals(tsOperations, rustOperations);
});

Deno.test("v0.3 preflight rows resolve real authorities and static artifacts", async () => {
  const report = await buildPreflightReport();
  assertEquals(report.scope, PREFLIGHT_SCOPE);
  assertEquals(report.generated_projection, true);
  assertEquals(report.authority, "not-authority");
  assertEquals(report.evidence_limit, "static-evidence-not-executed-proof");
  assertEquals(report.manifestsMatch, true);
  const mitaseSources = new Map<string, string>();
  for (
    const path of [
      "docs/mitase/requirements/search.yaml",
      "docs/mitase/requirements/api.yaml",
      "docs/mitase/requirements/forms.yaml",
      "docs/mitase/requirements/journey.yaml",
      "docs/mitase/requirements/frontend.yaml",
      "docs/mitase/features/api.yaml",
      "docs/mitase/features/journey.yaml",
    ]
  ) {
    mitaseSources.set(path, await Deno.readTextFile(path));
  }
  for (const row of report.rows) {
    for (const ref of row.requirement_criterion_refs) {
      assertEquals(
        resolveRequirementCriterionRef(ref, mitaseSources),
        true,
        ref,
      );
    }
    for (const ref of row.feature_binding_refs) {
      assertEquals(resolveFeatureBindingRef(ref, mitaseSources), true, ref);
    }
    for (const ref of row.verification_claim_refs) {
      assertEquals(resolveVerificationClaimRef(ref, mitaseSources), true, ref);
    }
  }
  assertEquals(
    report.rows.map((row) => row.id),
    PREFLIGHT_ROWS.map((row) => row.id),
  );
  for (const row of report.rows) {
    assertEquals(row.requirement_criterion_refs.length > 0, true, row.id);
    assertEquals(
      row.requirement_criterion_refs.every((ref) =>
        PREFLIGHT_ROWS.some((seed) =>
          seed.id === row.id && seed.requirement_criterion_refs.includes(ref)
        )
      ),
      true,
      `${row.id} has an undeclared criterion reference`,
    );
    assertEquals(
      (PREFLIGHT_EVIDENCE_STATUS as readonly string[]).includes(
        row.evidence_status,
      ),
      true,
      `${row.id} has an unknown evidence status`,
    );
    assertEquals(row.evidence_status === "executed-and-passed", false);
    assertEquals(
      new Set(row.operations ?? []).size,
      (row.operations ?? []).length,
      `${row.id} has duplicate operations`,
    );
    assertEquals(
      row.evidence_status,
      row.test_selectors.length > 0 ? "source-located" : "not-run",
      `${row.id} must not promote missing selectors or unrun tests`,
    );
    assertEquals(
      row.surface_observations.some((item) =>
        item.expected === "not-required" && !item.reason?.trim()
      ),
      false,
      `${row.id} has an unsupported not-required surface`,
    );
    for (const observation of row.surface_observations) {
      if (observation.observed) {
        assertEquals(observation.evidence.length > 0, true, row.id);
        assertEquals(
          observation.evidence.every((path) =>
            row.artifact_paths.includes(path)
          ),
          true,
          `${row.id} has an unregistered surface evidence path`,
        );
      }
    }
    const cliCoreObservation = row.surface_observations.find((item) =>
      item.surface === "CLI core"
    );
    const cliRemoteObservation = row.surface_observations.find((item) =>
      item.surface === "CLI remote"
    );
    if (cliCoreObservation && cliRemoteObservation) {
      assertEquals(
        cliCoreObservation.evidence.join(";") ===
          cliRemoteObservation.evidence.join(";"),
        false,
        `${row.id} reuses identical core and remote evidence`,
      );
    }
    assertEquals(row.reason.includes("Projection issues:"), false, row.id);
  }
  const markdown = renderPreflightMarkdown(report);
  assertEquals(markdown.includes("static evidence, not executed proof"), true);
  for (const row of report.rows) {
    assertEquals(markdown.includes(`| ${row.id} |`), true);
    assertEquals(markdown.includes(row.availability), true, row.id);
    assertEquals(markdown.includes(row.evidence_status), true);
    assertEquals(markdown.includes(row.reason), true, row.id);
    for (const ref of row.requirement_criterion_refs) {
      assertEquals(markdown.includes(ref), true, `${row.id} missing ${ref}`);
    }
    for (const ref of row.feature_binding_refs) {
      assertEquals(markdown.includes(ref), true, `${row.id} missing ${ref}`);
    }
    if (row.follow_up_issue) {
      assertEquals(markdown.includes(row.follow_up_issue), true, row.id);
    }
  }
});

Deno.test("preflight rejects malformed references, selectors, duplicates, and exceptions", async () => {
  const requirementSource = await Deno.readTextFile(
    "docs/mitase/requirements/search.yaml",
  );
  const featureSource = await Deno.readTextFile(
    "docs/mitase/features/journey.yaml",
  );
  const claimSource = await Deno.readTextFile(
    "docs/mitase/requirements/journey.yaml",
  );
  const sources = new Map([
    ["docs/mitase/requirements/search.yaml", requirementSource],
    ["docs/mitase/features/journey.yaml", featureSource],
    ["docs/mitase/requirements/journey.yaml", claimSource],
  ]);
  assertEquals(
    resolveRequirementCriterionRef(
      "REQ-SRCH-006#criterion.not-a-criterion",
      sources,
    ),
    false,
  );
  assertEquals(
    resolveFeatureBindingRef(
      "FEAT-JOURNEY-001#binding.frontend/target.not-a-target",
      sources,
    ),
    false,
  );
  assertEquals(
    resolveVerificationClaimRef(
      "docs/mitase/requirements/journey.yaml#REQ-JOURNEY-001/binding.journey-frontend-verification/not-a-target",
      sources,
    ),
    false,
  );
  assertEquals(
    resolveVerificationClaimRef(
      "docs/mitase/requirements/journey.yaml#REQ-JOURNEY-001/binding.journey-frontend-verification/form-case",
      sources,
      ["REQ-JOURNEY-001#criterion.entry-create"],
    ),
    false,
  );
  assertEquals(selectorLocated("test('present')", "missing"), false);

  const duplicate = {
    ...PREFLIGHT_ROWS[0],
    operations: ["entry.query", "entry.query"],
  };
  assertEquals(
    validatePreflightSeedShape(duplicate, ["entry.query"]).includes(
      "duplicate operation in row",
    ),
    true,
  );
  const unknownOperation = {
    ...PREFLIGHT_ROWS[0],
    operations: ["entry.not-real"],
  };
  assertEquals(
    validatePreflightSeedShape(unknownOperation, []).includes(
      "unknown operation: entry.not-real",
    ),
    true,
  );
  const unsupportedException = {
    ...PREFLIGHT_ROWS[2],
    surface_expectations: PREFLIGHT_ROWS[2].surface_expectations.map((item) =>
      item.expected === "not-required"
        ? { ...item, reason: "", evidence_paths: [] }
        : item
    ),
  };
  assertEquals(
    validatePreflightSeedShape(unsupportedException, []).includes(
      "not-required surface lacks evidence or a reason",
    ),
    true,
  );
});

// The seed covers the whole Golden journey and only journey checkpoints.
Deno.test("capability seed covers JOURNEY-KNOWLEDGE-001 checkpoints", () => {
  assertEquals(JOURNEY_ID, "JOURNEY-KNOWLEDGE-001");
  const checkpoints = JOURNEY_CAPABILITIES.map((capability) =>
    capability.checkpoint
  );
  assertEquals(checkpoints, [
    "Space",
    "Form",
    "Entry create",
    "Entry edit",
    "Search",
    "History",
    "Restore",
  ]);
  for (const capability of JOURNEY_CAPABILITIES) {
    assertEquals(capability.operations.length > 0, true);
    assertEquals(capability.requirement.includes("#criterion."), true);
    assertEquals(capability.outcome.length > 0, true);
  }
});

// State vocabulary distinguishes verified reachability from gaps.
Deno.test("capability states distinguish evidence and surface gaps", () => {
  assertEquals([...CAPABILITY_STATES], [
    "verified",
    "evidence-gap",
    "surface-gap",
    "semantic-drift",
    "intentionally-not-required",
  ]);
  const verified = classify(
    { frontend: true, cliCore: true, cliRemote: true },
    true,
    [],
    ["docs/mitase/requirements/journey.yaml verifies REQ-JOURNEY-001#criterion.form-establish"],
  );
  assertEquals(verified.state, "verified");
  const missing = classify(
    { frontend: true, cliCore: false, cliRemote: true },
    true,
    [],
    ["some claim"],
  );
  assertEquals(missing.state, "surface-gap");
  const drift = classify(
    { frontend: true, cliCore: true, cliRemote: true },
    false,
    [],
    ["some claim"],
  );
  assertEquals(drift.state, "semantic-drift");
});

// CLI core service methods must stay mapped to the operations they fulfil.
Deno.test("CLI core service map covers the journey mutations", () => {
  assertEquals(SERVICE_METHOD_OPERATIONS["upsert_form"], "form.upsert");
  assertEquals(SERVICE_METHOD_OPERATIONS["create_entry"], "entry.create");
  assertEquals(SERVICE_METHOD_OPERATIONS["update_entry"], "entry.update");
  assertEquals(SERVICE_METHOD_OPERATIONS["entry_history"], "entry.history");
  assertEquals(SERVICE_METHOD_OPERATIONS["entry_revision"], "entry.revision");
  assertEquals(SERVICE_METHOD_OPERATIONS["restore_entry"], "entry.restore");
  assertEquals(SERVICE_METHOD_OPERATIONS["query_entry_page"], "entry.query");
});

// Informational report: gaps are reported, never fail the build.
Deno.test("capability report projects every journey capability", async () => {
  const report = await buildReport();
  assertEquals(report.journey, "JOURNEY-KNOWLEDGE-001");
  assertEquals(report.manifestsMatch, true);
  assertEquals(report.rustOnly, []);
  assertEquals(report.tsOnly, []);
  assertEquals(
    report.capabilities.map((row) => row.id),
    JOURNEY_CAPABILITIES.map((capability) => capability.id),
  );
  for (const row of report.capabilities) {
    assertEquals(
      (CAPABILITY_STATES as readonly string[]).includes(row.state),
      true,
      `${row.id} has an unknown state`,
    );
  }
  const byId = new Map(report.capabilities.map((row) => [row.id, row]));
  // Form establish is reachable through each surface's normal save flow and
  // uses the journey criterion that declares its cross-surface evidence.
  assertEquals(byId.get("form-establish")?.frontend, true);
  assertEquals(byId.get("form-establish")?.cliCore, true);
  assertEquals(byId.get("form-establish")?.cliRemote, true);
  assertEquals(byId.get("form-establish")?.state, "verified");
  // Restore keeps its history semantics with exact journey evidence wired.
  assertEquals(byId.get("entry-restore")?.state, "verified");
  const markdown = renderMarkdown(report);
  assertEquals(markdown.includes("JOURNEY-KNOWLEDGE-001"), true);
  assertEquals(markdown.includes("form-establish"), true);
  assertEquals(markdown.includes("entry-restore"), true);
});

// Mitase declares exact verification targets but never executes tests, so
// selector/test-name parity is pinned here instead of failing silently.
Deno.test("journey Mitase selectors match exact Playwright test names", async () => {
  const testSource = await Deno.readTextFile("e2e/knowledge-journey.test.ts");
  const yaml = await Deno.readTextFile("docs/mitase/requirements/journey.yaml");
  const declared = [...yaml.matchAll(/name: '([^']+)'/g)]
    .map((match) => match[1])
    .filter((name) => name.startsWith("JOURNEY-"));
  assertEquals(declared.length, 12);
  for (const name of declared) {
    assertEquals(
      testSource.includes(`test("${name}"`),
      true,
      `missing Playwright case ${name}`,
    );
  }
});

// Same parity rule for the Rust corpus: every bare `name:` selector in the
// journey requirements must resolve to a real test function in its harness.
Deno.test("parity Mitase selectors match exact Rust test functions", async () => {
  const yaml = await Deno.readTextFile("docs/mitase/requirements/journey.yaml");
  const selectors = [...yaml.matchAll(
    /path: (crates\/ugoite-cli\/tests\/test_journey_(?:core|remote)\.rs)\n\s*selector: \{ kind: test, name: ([a-z0-9_]+) \}/g,
  )].map((match) => ({ file: match[1], name: match[2] }));
  assertEquals(selectors.length, 17);
  const sources = new Map<string, string>();
  for (const { file, name } of selectors) {
    if (!sources.has(file)) sources.set(file, await Deno.readTextFile(file));
    const source = sources.get(file) as string;
    assertEquals(
      source.includes(`fn ${name}(`),
      true,
      `missing Rust test ${name} in ${file}`,
    );
  }
});
