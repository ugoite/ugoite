import { assert, assertEquals } from "@std/assert";

type Selector =
  | { path: string; kind: "file" }
  | { path: string; kind: "test"; name: string };

type Criterion = {
  id: string;
  requirement: string;
  selectors: Selector[];
  manual_evidence_gaps?: string[];
};

type AcceptancePlan = {
  schema: string;
  source: string;
  criteria: Criterion[];
};

const PLAN_PATH = "e2e/fixtures/composition/acceptance-plan.json";
const EXPECTED_IDS = Array.from(
  { length: 14 },
  (_, index) => `A${String(index + 1).padStart(2, "0")}`,
);
const EXPECTED_REQUIREMENTS = [
  "Composition schema: entry_query, tags, and every label field make YAML invalid; there is no compatibility parser.",
  'Form to SQL: selecting Form G generates SELECT * FROM "G"; SQL query limit=50 is separate from SQL text LIMIT.',
  "Source binding: Form rename follows immutable identity; same-name Form replacement fails; ACL revocation or a missing Form stops with source_unavailable.",
  "Saved SQL: pin an exact revision with no latest fallback, silent rebind, or mutation; duplication creates inline SQL.",
  "Variables: Composition defaults match their type; required, unset, and invalid values remain distinct; SQL is never interpolated. Saved SQL defaults are typed in standalone preview.",
  "SQL functions: read-only SQL supports COUNT, SUM, AVG, MIN, MAX and existing join/group/distinct parsing; the UI does not unconditionally generate disallowed scalar or window functions.",
  "SQL editing: GUI actions never overwrite hand-edited SQL without consent; unsupported complex SQL remains editable, saveable, and validatable in the code editor.",
  "Query paging: only explicit sql.query.count counts; continuations recheck authorization; stale query/result is suppressed; the client does not aggregate all pages.",
  "Metric: distinguish zero rows, one NULL row, zero, multiple rows, and type mismatch; derive headings from SQL result aliases; persist no label.",
  "Composition Studio: test 320, 375, 390, 760, and 1280px across available modes; no page horizontal overflow; mode targets are at least 44px; keyboard focus remains visible at every tested width; touch selection, light/dark mode, and 16px input text work. Verify actual 200% browser zoom and iPhone Safari separately.",
  "UI: no persistent explanatory paragraphs, decorative cards, or tag/label editors; exactly one Back and one Save action; selecting a row selects the whole row.",
  "Accessibility: every icon-only control has aria-label/title and visible focus, targets are at least 44px, drawers trap and restore focus, and dialogs support keyboard and Escape. Verify VoiceOver/NVDA separately.",
  "Recovery: one Composition Entry revision per save; return receipts; stale base revisions conflict; distinguish unknown outcomes. Undo appends a publication and never rewinds the head destructively.",
  "CLI and WASM: share the Composition schema, resolution, diagnostics, and SQL preview semantics; raw export/history remain usable with no legacy executable decoder.",
];

Deno.test(
  "Composition acceptance plan preserves all A01–A14 requirements and points to evidence",
  async () => {
    const raw = JSON.parse(await Deno.readTextFile(PLAN_PATH)) as Record<
      string,
      unknown
    >;
    const plan = raw as AcceptancePlan;
    assertEquals(plan.schema, "ugoite/composition-acceptance-plan/v1");
    assertEquals(
      plan.source,
      "2026-10-21 implementation directive: PR1 pauses incomplete Composition browser routes; PR3 restores the completed Studio",
    );
    assertEquals(plan.criteria.map(({ id }) => id), EXPECTED_IDS);
    assertEquals(
      plan.criteria.map(({ requirement }) => requirement),
      EXPECTED_REQUIREMENTS,
    );

    for (
      const key of ["status", "runtime_evidence", "release_status"] as const
    ) {
      assert(!(key in raw), `acceptance plan must not infer ${key}`);
    }

    for (const criterion of plan.criteria) {
      assert(criterion.selectors.length > 0, `${criterion.id} needs evidence`);
      for (const selector of criterion.selectors) {
        const source = await Deno.readTextFile(selector.path);
        assert(source.length > 0, `${criterion.id}: ${selector.path}`);
        if (selector.kind === "test") {
          assert(
            source.includes(selector.name),
            `${criterion.id}: missing selector ${selector.name}`,
          );
        }
      }
    }

    assertEquals(
      plan.criteria.find(({ id }) => id === "A10")?.manual_evidence_gaps,
      [
        "Composition browser routes are paused until PR3; browser UI gates have no route-level evidence.",
        "Actual browser 200% zoom and iPhone Safari device verification",
      ],
    );
    assertEquals(
      plan.criteria.find(({ id }) => id === "A02")?.manual_evidence_gaps,
      [
        "Composition browser routes are paused until PR3; Form authoring E2E will be restored there.",
        "Browser verification of the preview cap versus SQL text LIMIT is deferred.",
      ],
    );
    assert(
      plan.criteria.find(({ id }) => id === "A09")?.selectors.some((selector) =>
        selector.path === "e2e/composition-metric-journey.test.ts" &&
        selector.kind === "test" &&
        selector.name ===
          "API evaluates an exact Saved SQL revision and reports scalar cardinality failures"
      ),
      "A09 must retain request-level exact revision and cardinality evidence",
    );
    const removedRouteTests = new Set([
      "e2e/composition-golden-journey.test.ts",
      "e2e/composition-invalid-variable-input.test.ts",
      "e2e/composition-revision-stale-response.test.ts",
      "e2e/composition-studio-journey.test.ts",
    ]);
    assert(
      plan.criteria.every((criterion) =>
        criterion.selectors.every((selector) =>
          !removedRouteTests.has(selector.path)
        )
      ),
      "paused Composition UI tests must not be presented as current evidence",
    );
    assertEquals(
      plan.criteria.find(({ id }) => id === "A12")?.manual_evidence_gaps,
      [
        "Composition browser routes are paused until PR3; dialog and drawer browser behavior is not currently available for verification.",
        "VoiceOver and NVDA device verification",
      ],
    );
    assert(
      plan.criteria.every((criterion) =>
        criterion.manual_evidence_gaps === undefined ||
        criterion.manual_evidence_gaps.length > 0
      ),
      "manual evidence gaps are explicit when present",
    );
  },
);
