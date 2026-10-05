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
