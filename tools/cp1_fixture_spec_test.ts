import { assertEquals } from "@std/assert/equals";
import {
  CP1_FIXTURE_SPEC_SCHEMA_VERSION,
  cp1FixtureBySlug,
  cp1FixtureRows,
  cp1FixturesFor,
} from "./cp1_fixture_spec.ts";

Deno.test("CP1 fixture specification preserves the query and export contracts", () => {
  assertEquals(CP1_FIXTURE_SPEC_SCHEMA_VERSION, 1);
  assertEquals(
    cp1FixturesFor("query").map((fixture) => ({
      slug: fixture.slug,
      scenario: fixture.scenario,
      seed: fixture.seed,
      entryCount: fixture.entryCount,
      ownerDisplayName: fixture.ownerDisplayName,
    })),
    [
      {
        slug: "query-space-a",
        scenario: "renewable-ops",
        seed: 3134001,
        entryCount: 6000,
        ownerDisplayName: "Query Measurement Owner",
      },
      {
        slug: "query-space-b",
        scenario: "renewable-ops",
        seed: 3134002,
        entryCount: 4000,
        ownerDisplayName: "Query Measurement Owner",
      },
    ],
  );
  assertEquals(
    cp1FixturesFor("export").map((fixture) => ({
      slug: fixture.slug,
      scenario: fixture.scenario,
      seed: fixture.seed,
      entryCount: fixture.entryCount,
      ownerDisplayName: fixture.ownerDisplayName,
    })),
    [{
      slug: "sql-export-measure",
      scenario: "renewable-ops",
      seed: 3140001,
      entryCount: 10000,
      ownerDisplayName: null,
    }],
  );
  assertEquals(cp1FixtureBySlug("query-space-a"), cp1FixturesFor("query")[0]);
  assertEquals(
    cp1FixtureRows("query").split("\n"),
    [
      "query-space-a\trenewable-ops\t3134001\t6000\tQuery Measurement Owner",
      "query-space-b\trenewable-ops\t3134002\t4000\tQuery Measurement Owner",
    ],
  );
  assertEquals(
    cp1FixtureRows("export"),
    "sql-export-measure\trenewable-ops\t3140001\t10000\t",
  );
});

Deno.test("CP1 fixture slugs and seeds are unique", () => {
  const fixtures = [...cp1FixturesFor("query"), ...cp1FixturesFor("export")];
  assertEquals(
    new Set(fixtures.map((fixture) => fixture.slug)).size,
    fixtures.length,
  );
  assertEquals(
    new Set(fixtures.map((fixture) => fixture.seed)).size,
    fixtures.length,
  );
  let unknownSlugError = "";
  try {
    cp1FixtureBySlug("unlisted-space");
  } catch (error) {
    unknownSlugError = error instanceof Error ? error.message : String(error);
  }
  assertEquals(unknownSlugError, "unknown CP1 fixture slug: unlisted-space");
});
