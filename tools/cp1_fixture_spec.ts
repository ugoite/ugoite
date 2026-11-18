export type Cp1FixtureSet = "query" | "export";

export type Cp1Fixture = Readonly<{
  slug: string;
  scenario: "renewable-ops";
  seed: number;
  entryCount: number;
  ownerDisplayName: string | null;
  formNames: readonly string[];
}>;

/**
 * Maximum Entries per seeder mutation batch. Mirrors the production
 * `entry::MAX_ENTRY_CREATE_BATCH_SIZE` bound enforced by the sample-data
 * seeder; fixture profile validators use it to reject impossible batch
 * distributions before a bundle is published or consumed.
 */
export const CP1_SEED_MUTATION_BATCH_LIMIT = 256;
const CP1_RENEWABLE_OPS_FORM_NAMES = [
  "Array",
  "EnergyReport",
  "Entry",
  "Inspection",
  "MaintenanceTicket",
  "Site",
] as const;

const CP1_FIXTURE_SETS: Readonly<
  Record<
    Cp1FixtureSet,
    readonly (Cp1Fixture & { measurementEntryCount: number })[]
  >
> = {
  query: [
    {
      slug: "query-space-a",
      scenario: "renewable-ops",
      seed: 3134001,
      // The 25% MaintenanceTicket share yields one row beyond the Browser's
      // 100-row SQL page. The admission and lifecycle checks need only two pages.
      entryCount: 404,
      measurementEntryCount: 1200,
      ownerDisplayName: "Query Measurement Owner",
      formNames: CP1_RENEWABLE_OPS_FORM_NAMES,
    },
    {
      slug: "query-space-b",
      scenario: "renewable-ops",
      seed: 3134002,
      // Space switching needs rows in both Spaces. Twelve Entries populate
      // all five scenario Forms and preserve the exact 25% ticket share.
      entryCount: 12,
      measurementEntryCount: 800,
      ownerDisplayName: "Query Measurement Owner",
      formNames: CP1_RENEWABLE_OPS_FORM_NAMES,
    },
  ],
  export: [
    {
      slug: "sql-export-measure",
      scenario: "renewable-ops",
      seed: 3140001,
      // One row per scenario Form, enough to compare single- and multi-page export.
      entryCount: 5,
      measurementEntryCount: 1000,
      ownerDisplayName: null,
      formNames: CP1_RENEWABLE_OPS_FORM_NAMES,
    },
  ],
};

type FixtureMode = "acceptance" | "measurement";

export function cp1FixturesFor(
  set: Cp1FixtureSet,
  mode: FixtureMode = "acceptance",
): readonly Cp1Fixture[] {
  return CP1_FIXTURE_SETS[set].map(({ measurementEntryCount, ...fixture }) => ({
    ...fixture,
    entryCount: mode === "measurement"
      ? measurementEntryCount
      : fixture.entryCount,
  }));
}

export function cp1FixtureBySlug(
  slug: string,
  mode: FixtureMode = "acceptance",
): Cp1Fixture {
  for (const set of ["query", "export"] as const) {
    const fixture = cp1FixturesFor(set, mode).find((candidate) =>
      candidate.slug === slug
    );
    if (fixture) return fixture;
  }
  throw new Error("unknown CP1 fixture slug: " + slug);
}

export function cp1FixtureRows(set: Cp1FixtureSet, mode?: FixtureMode): string {
  return cp1FixturesFor(set, mode).map((fixture) =>
    [
      fixture.slug,
      fixture.scenario,
      fixture.seed,
      fixture.entryCount,
      fixture.ownerDisplayName ?? "",
    ].join("\t")
  ).join("\n");
}

if (import.meta.main) {
  const [setName, measurement, ...extra] = Deno.args;
  if (
    (setName !== "query" && setName !== "export") ||
    (measurement !== undefined && measurement !== "--measurement") ||
    extra.length > 0
  ) {
    throw new Error(
      "usage: cp1_fixture_spec.ts <query|export> [--measurement]",
    );
  }
  console.log(
    cp1FixtureRows(setName, measurement ? "measurement" : "acceptance"),
  );
}
