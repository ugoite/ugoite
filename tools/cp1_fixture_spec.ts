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

/**
 * Fixed CP1 acceptance fixture sizes.
 *
 * Rescaled 2026-09-30 from 6,000 + 4,000 (query) and 10,000 (export) to
 * 1,200 + 800 and 1,000: seed wall time scales with entry volume and the
 * fixture lane dominated the CP1 critical path. The smaller sizes preserve
 * every structural property the consumers assert:
 * - two distinct query Spaces with the same 6:4 size ratio, seeds, scenario,
 *   and owner;
 * - totals divisible by 4 so the MaintenanceTicket share (25% of the
 *   renewable-ops distribution) stays an exact count;
 * - more than 100 MaintenanceTicket rows per query Space so the browser
 *   page-size-100 pagination reaches Page 2 with a continuation;
 * - multi-batch seeds (256-entry mutation batches) for query and export;
 * - export totals divisible by both page sizes (100 x 10 pages, 1,000 x 1).
 * Trial counts, page sizes, lifecycle assertions, and the zero-skip policy
 * are unchanged. Slugs, seeds, scenario, and owners are unchanged.
 *
 * Sizing policy: the required lane uses only these minimal structural
 * fixtures. Larger fixed-count volumes stay outside the required lane as
 * scheduled or profile-only measurements; never add a fixed large-count
 * release contract here. See `docs/architecture/testing/ci-cd.md`.
 */

/**
 * Maximum Entries per seeder mutation batch. Mirrors the production
 * `entry::MAX_ENTRY_CREATE_BATCH_SIZE` bound enforced by the sample-data
 * seeder; fixture profile validators use it to reject impossible batch
 * distributions before a bundle is published or consumed.
 */
export const CP1_SEED_MUTATION_BATCH_LIMIT = 256;
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
      entryCount: 1200,
      ownerDisplayName: "Query Measurement Owner",
      formNames: CP1_RENEWABLE_OPS_FORM_NAMES,
    },
    {
      slug: "query-space-b",
      scenario: "renewable-ops",
      seed: 3134002,
      entryCount: 800,
      ownerDisplayName: "Query Measurement Owner",
      formNames: CP1_RENEWABLE_OPS_FORM_NAMES,
    },
  ],
  export: [
    {
      slug: "sql-export-measure",
      scenario: "renewable-ops",
      seed: 3140001,
      entryCount: 1000,
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
