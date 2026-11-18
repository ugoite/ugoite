import { assertEquals } from "@std/assert/equals";
import { assertThrows } from "@std/assert/throws";
import {
  cp1FixtureBySlug,
  cp1FixtureRows,
  cp1FixturesFor,
} from "./cp1_fixture_spec.ts";

Deno.test("CP1 fixture CLI defaults to structural counts and opts in to measurement volumes", () => {
  for (
    const [kind, acceptance, measurement] of [
      ["query", [404, 12], [1200, 800]],
      ["export", [5], [1000]],
    ] as const
  ) {
    const entryCounts = (rows: string) =>
      rows.split("\n").map((row) => Number(row.split("\t")[3]));
    assertEquals(entryCounts(cp1FixtureRows(kind)), [...acceptance]);
    assertEquals(
      entryCounts(cp1FixtureRows(kind, "measurement")),
      [...measurement],
    );
  }
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
  assertThrows(
    () => cp1FixtureBySlug("unlisted-space"),
    Error,
    "unknown CP1 fixture slug: unlisted-space",
  );
});
