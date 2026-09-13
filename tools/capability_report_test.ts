import { assertEquals } from "@std/assert/equals";
import {
  buildReport,
  CAPABILITY_STATES,
  classify,
  JOURNEY_CAPABILITIES,
  JOURNEY_ID,
  renderMarkdown,
  rustManifestOperations,
  SERVICE_METHOD_OPERATIONS,
  typescriptManifestOperations,
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
  assertEquals(rustOperations.length, 67);
  assertEquals(tsOperations, rustOperations);
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

// State vocabulary keeps alias and absence distinct by construction.
Deno.test("capability states distinguish discoverability from absence", () => {
  assertEquals([...CAPABILITY_STATES], [
    "verified",
    "evidence-gap",
    "surface-gap",
    "semantic-drift",
    "implemented-undiscoverable",
    "intentionally-not-required",
  ]);
  const aliased = classify(
    { frontend: true, cliCore: true, cliRemote: true },
    true,
    "CLI reaches form.upsert via `form update`",
    [],
    ["docs/mitase/requirements/forms.yaml verifies REQ-FORM-010#criterion.upsert-operation"],
  );
  assertEquals(aliased.state, "implemented-undiscoverable");
  const missing = classify(
    { frontend: true, cliCore: false, cliRemote: true },
    true,
    undefined,
    [],
    ["some claim"],
  );
  assertEquals(missing.state, "surface-gap");
  const drift = classify(
    { frontend: true, cliCore: true, cliRemote: true },
    false,
    undefined,
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
  assertEquals(SERVICE_METHOD_OPERATIONS["search_entries"], "search.keyword");
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
  // Form establish is reachable everywhere but hides behind `form update`.
  assertEquals(byId.get("form-establish")?.frontend, true);
  assertEquals(byId.get("form-establish")?.cliCore, true);
  assertEquals(byId.get("form-establish")?.cliRemote, true);
  assertEquals(
    byId.get("form-establish")?.state,
    "implemented-undiscoverable",
  );
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
  assertEquals(selectors.length, 15);
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
