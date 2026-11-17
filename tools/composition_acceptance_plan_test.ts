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
  { length: 16 },
  (_, index) => `A${String(index + 1).padStart(2, "0")}`,
);
const EXPECTED_REQUIREMENTS = [
  "Current Composition documents save a strict typed graph with inline SQL and only Text, Table, and Input components.",
  "Form-backed Table results use explicit projected SQL columns in query order, and query count remains separate from paged result reads.",
  "Inline SQL retains its stable Form identity, while evaluation rechecks current Form authorization.",
  "Published Saved SQL remains a separate usable path; Composition query variables store inline SQL and do not bind Saved SQL revisions.",
  "Typed variable defaults and required, unset, and invalid input states remain distinct; SQL parameters bind explicitly without SQL interpolation.",
  "Scalar SQL results render through plain Text; the current Composition graph has no Metric component or persisted label.",
  "A query keeps one authored SQL document; lint, authorized schema check, and explicit execution remain separate controls.",
  "Table paging uses explicit count and bounded pages; continuation authorization is rechecked and stale query results are suppressed.",
  "Scalar queries require one output column and one row; zero rows, NULL, zero, multiple rows, and type drift remain distinct outcomes.",
  "At 320, 375, and 390px, a read-only Table keeps horizontal scrolling inside its own region, exposes its final projected column, preserves scalar Text visibility, and does not widen the page.",
  "Design offers Text, Table, and Input through concise controls; duplicate variable names are rejected before a query is added.",
  "Keyboard users can reach Studio controls with visible focus; the Table picker restores focus on Escape and icon controls have accessible names.",
  "Save receipts, stale-revision conflicts, exact revision reads, and raw Composition history preserve the append-only recovery contract.",
  "CLI and WASM use shared Rust Composition semantics; raw exact reads, export, and history do not require executing replaced prerelease shapes.",
  "Authors edit Text in place with multiline and IME-safe keyboard behavior; readers preserve newlines and render displayable scalar values as escaped plain text.",
  "Saved Composition revisions recheck current Entry access after query results are staged and before results are returned.",
];

Deno.test(
  "Composition acceptance plan maps current Text-first criteria to repository evidence",
  async () => {
    const raw = JSON.parse(await Deno.readTextFile(PLAN_PATH)) as Record<
      string,
      unknown
    >;
    const plan = raw as AcceptancePlan;
    assertEquals(plan.schema, "ugoite/composition-acceptance-plan/v1");
    assertEquals(
      plan.source,
      "Current Text-first Composition acceptance evidence (ADR-027)",
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
      plan.criteria.find(({ id }) => id === "A03")?.manual_evidence_gaps,
      [
        "Browser evidence for renaming a Form and recreating a same-name Form remains unavailable because the current API has no rename or delete path; native Rust coverage verifies stable identity.",
      ],
    );
    const manualGaps = plan.criteria.flatMap((criterion) =>
      criterion.manual_evidence_gaps ?? []
    );
    assert(
      !manualGaps.some((gap) =>
        /physical.?device|iPhone Safari|actual 200%|VoiceOver|NVDA/i.test(gap)
      ),
      "physical-device, native-zoom, and manual screen-reader checks are not acceptance gaps",
    );
    for (const id of ["A02", "A03", "A04", "A08", "A10", "A11", "A12", "A13"]) {
      assert(
        plan.criteria.find((criterion) => criterion.id === id)?.selectors.some((
          selector,
        ) => selector.path.startsWith("e2e/composition-")),
        `${id} needs current browser evidence`,
      );
    }
    assert(
      plan.criteria.every((criterion) =>
        criterion.manual_evidence_gaps === undefined ||
        criterion.manual_evidence_gaps.length > 0
      ),
      "manual evidence gaps are explicit when present",
    );
  },
);
