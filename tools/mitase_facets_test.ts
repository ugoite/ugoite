import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url).pathname;
const scriptPath = new URL("../scripts/mitase", import.meta.url).pathname;

async function reportFacets(
  source: string,
  format = "json",
): Promise<{ success: boolean; code: number; stdout: string; stderr: string }> {
  const output = await new Deno.Command("bash", {
    args: [scriptPath, "report", "facets", source, ".", "--format", format],
    cwd: root,
    stdout: "piped",
    stderr: "piped",
  }).output();
  const decode = new TextDecoder();
  return {
    success: output.success,
    code: output.code,
    stdout: decode.decode(output.stdout),
    stderr: decode.decode(output.stderr),
  };
}

Deno.test("report facets projects the pinned specification contract", async () => {
  const result = await reportFacets("FEAT-ENTRY-001");
  assertEquals(result.success, true, result.stderr);
  const report = JSON.parse(result.stdout);
  assertEquals(report.schema_version, "mitase/cli/v1");
  assertEquals(
    report.contract_version,
    "mitase/facet-projection-report/v1",
  );
  assertEquals(report.source, "FEAT-ENTRY-001");
  assertEquals(report.source_kind, "feature");
  assertEquals(Array.isArray(report.criteria), true);
  assertEquals(Array.isArray(report.non_semantic_targets), true);
});

Deno.test("report facets resolves a criterion source", async () => {
  const result = await reportFacets("REQ-ENTRY-001#criterion.creation");
  assertEquals(result.success, true, result.stderr);
  const report = JSON.parse(result.stdout);
  assertEquals(report.source_kind, "criterion");
  const entry = report.criteria.find(
    (entry: { criterion: string }) =>
      entry.criterion === "REQ-ENTRY-001#criterion.creation",
  );
  assertEquals(typeof entry, "object");
  assertEquals(entry.facets.length > 0, true);
  for (const row of entry.facets) {
    assertEquals(typeof row.facet, "string");
    assertEquals(typeof row.feature, "string");
    assertEquals(typeof row.binding, "string");
  }
});

Deno.test("report facets rejects unsupported sources without an envelope", async () => {
  const result = await reportFacets("REQ-ENTRY-001");
  assertEquals(result.success, false);
  assertEquals(result.code, 2);
  assertEquals(result.stdout, "");
  assertEquals(result.stderr.includes("facet projection source"), true);
});

Deno.test("entry capability projects one outcome across honest facets", async () => {
  const result = await reportFacets("FEAT-ENTRY-001");
  assertEquals(result.success, true, result.stderr);
  const report = JSON.parse(result.stdout);
  const byCriterion = new Map(
    report.criteria.map((entry: { criterion: string; facets: unknown[] }) => [
      entry.criterion,
      entry.facets,
    ]),
  );
  const creation = byCriterion.get("REQ-ENTRY-001#criterion.creation") as Array<
    {
      facet: string;
      declared_verification: { status: string };
    }
  >;
  assertEquals(
    creation.map((row) => row.facet),
    ["backend", "cli-core", "cli-remote", "core", "frontend", "mcp"],
  );
  for (const row of creation) {
    assertEquals(row.declared_verification.status, "verified", row.facet);
  }
  const tombstone = byCriterion.get(
    "REQ-ENTRY-004#criterion.tombstone",
  ) as Array<{
    facet: string;
  }>;
  const tombstoneFacets = tombstone.map((row) => row.facet).sort();
  assertEquals(tombstoneFacets.includes("cli-remote"), false);
  assertEquals(tombstoneFacets.includes("mcp"), false);
});
