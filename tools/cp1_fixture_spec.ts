export type Cp1FixtureSet = "query" | "export";

export type Cp1Fixture = Readonly<{
  slug: string;
  scenario: "renewable-ops";
  seed: number;
  entryCount: number;
  ownerDisplayName: string | null;
  formNames: readonly string[];
}>;

export const CP1_FIXTURE_SPEC_SCHEMA_VERSION = 1;
export const CP1_RENEWABLE_OPS_FORM_NAMES = [
  "Array",
  "EnergyReport",
  "Entry",
  "Inspection",
  "MaintenanceTicket",
  "Site",
] as const;

export const CP1_FIXTURE_SETS: Readonly<
  Record<Cp1FixtureSet, readonly Cp1Fixture[]>
> = {
  query: [
    {
      slug: "query-space-a",
      scenario: "renewable-ops",
      seed: 3134001,
      entryCount: 6000,
      ownerDisplayName: "Query Measurement Owner",
      formNames: CP1_RENEWABLE_OPS_FORM_NAMES,
    },
    {
      slug: "query-space-b",
      scenario: "renewable-ops",
      seed: 3134002,
      entryCount: 4000,
      ownerDisplayName: "Query Measurement Owner",
      formNames: CP1_RENEWABLE_OPS_FORM_NAMES,
    },
  ],
  export: [
    {
      slug: "sql-export-measure",
      scenario: "renewable-ops",
      seed: 3140001,
      entryCount: 10000,
      ownerDisplayName: null,
      formNames: CP1_RENEWABLE_OPS_FORM_NAMES,
    },
  ],
};

export function cp1FixturesFor(set: Cp1FixtureSet): readonly Cp1Fixture[] {
  return CP1_FIXTURE_SETS[set];
}

export function cp1FixtureBySlug(slug: string): Cp1Fixture {
  for (const fixtures of Object.values(CP1_FIXTURE_SETS)) {
    const fixture = fixtures.find((candidate) => candidate.slug === slug);
    if (fixture) return fixture;
  }
  throw new Error("unknown CP1 fixture slug: " + slug);
}

export function cp1FixtureRows(set: Cp1FixtureSet): string {
  return cp1FixturesFor(set).map((fixture) =>
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
  const [setName, ...extra] = Deno.args;
  if (
    (setName !== "query" && setName !== "export") ||
    extra.length > 0
  ) {
    throw new Error(
      "usage: cp1_fixture_spec.ts <query|export>",
    );
  }
  console.log(cp1FixtureRows(setName));
}
