import { assertEquals } from "@std/assert/equals";
import { PREFLIGHT_ROWS } from "./capability_report.ts";

const root = new URL("../", import.meta.url).pathname;
const scriptPath = new URL("../scripts/mitase", import.meta.url).pathname;

async function mitase(
  args: string[],
): Promise<{ code: number; stdout: string; stderr: string }> {
  const output = await new Deno.Command("bash", {
    args: [scriptPath, ...args],
    cwd: root,
    stdout: "piped",
    stderr: "piped",
  }).output();
  const decode = new TextDecoder();
  return {
    code: output.code,
    stdout: decode.decode(output.stdout),
    stderr: decode.decode(output.stderr),
  };
}

Deno.test("capability preflight refs resolve in the Mitase graph", async () => {
  const row = PREFLIGHT_ROWS.find((seed) =>
    seed.id === "knowledge-mutation-recovery"
  );
  assertEquals(typeof row, "object");
  for (const ref of row!.feature_binding_refs) {
    const result = await mitase(["query", ref, ".", "--format", "json"]);
    assertEquals(result.code, 0, `${ref}: ${result.stderr}`);
    const report = JSON.parse(result.stdout);
    assertEquals(report.source, ref);
  }
});

Deno.test("capability preflight refs match declared facet verification", async () => {
  const result = await mitase([
    "report",
    "facets",
    "FEAT-ENTRY-001",
    ".",
    "--format",
    "json",
  ]);
  assertEquals(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assertEquals(report.contract_version, "mitase/facet-projection-report/v1");
  const byCriterion = new Map(
    report.criteria.map((entry: { criterion: string; facets: unknown[] }) => [
      entry.criterion,
      entry.facets,
    ]),
  );
  const creation = byCriterion.get("REQ-ENTRY-001#criterion.creation") as Array<
    {
      facet: string;
      implementation_targets: string[];
      declared_verification: { status: string };
    }
  >;
  const frontend = creation.find((facetRow) => facetRow.facet === "frontend");
  assertEquals(typeof frontend, "object");
  assertEquals(
    frontend!.implementation_targets,
    ["FEAT-ENTRY-001#binding.frontend-implementation/target.entry-api"],
  );
  assertEquals(frontend!.declared_verification.status, "verified");
});

Deno.test("required surface expectations match the facet projection", async () => {
  // Project-owned mapping from capability-report surface names to Mitase
  // opaque facets. Required/not-required stays owned by PREFLIGHT_ROWS;
  // this test detects drift between that expectation and the Mitase graph.
  // Statuses here are declared verification, never executed-and-passed.
  const expected: Record<string, "verified" | "absent"> = {
    frontend: "verified",
    "cli-core": "verified",
    "cli-remote": "verified",
    mcp: "verified",
    "client-host": "absent",
  };
  const result = await mitase([
    "report",
    "facets",
    "FEAT-ENTRY-001",
    ".",
    "--format",
    "json",
  ]);
  assertEquals(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  const creation = report.criteria.find(
    (entry: { criterion: string }) =>
      entry.criterion === "REQ-ENTRY-001#criterion.creation",
  );
  const byFacet = new Map(
    creation.facets.map(
      (
        facetRow: { facet: string; declared_verification: { status: string } },
      ) => [facetRow.facet, facetRow.declared_verification.status],
    ),
  );
  for (const [facet, expectation] of Object.entries(expected)) {
    if (expectation === "absent") {
      assertEquals(
        byFacet.has(facet),
        false,
        `${facet} has no entry implementation; the Konase Host blank stays explicit while availability is unresolved`,
      );
    } else {
      assertEquals(
        byFacet.get(facet),
        "verified",
        `${facet} drifts from its required expectation`,
      );
    }
  }
});
