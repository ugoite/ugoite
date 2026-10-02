import { assert, assertEquals } from "@std/assert";

type ReservedSelector = {
  path: string;
  selector: { kind: "test"; name: string } | { kind: "file" };
};

type Milestone = {
  id: string;
  status: string;
  outcome: string;
  reserved_selectors: ReservedSelector[];
};

type AcceptancePlan = {
  schema: string;
  status: string;
  implementation_contract_frozen: boolean;
  runtime_evidence_recorded: boolean;
  fixture_layout: {
    root: string;
    e2e_payloads_created: boolean;
    shared_document_fixtures: string[];
    planned_files: string[];
  };
  operation_inventory: string[];
  cli_commands: string[];
  browser_golden_journey: {
    model_connection: string;
    steps: string[];
  };
  evidence_record_fields: string[];
  milestones: Milestone[];
};

const PLAN_PATH = "e2e/fixtures/composition/acceptance-plan.json";

Deno.test(
  "Composition acceptance plan covers B0 through B6 without claiming runtime verification",
  async () => {
    const plan = JSON.parse(
      await Deno.readTextFile(PLAN_PATH),
    ) as AcceptancePlan;

    assertEquals(plan.schema, "ugoite/composition-acceptance-plan/v1");
    assertEquals(plan.status, "planned");
    assertEquals(plan.implementation_contract_frozen, false);
    assertEquals(plan.runtime_evidence_recorded, false);
    assertEquals(plan.fixture_layout.root, "e2e/fixtures/composition");
    assertEquals(plan.fixture_layout.e2e_payloads_created, false);
    assertEquals(
      plan.fixture_layout.shared_document_fixtures[0],
      "crates/ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml",
    );

    assertEquals(plan.operation_inventory, [
      "composition.list",
      "composition.get",
      "composition.lint",
      "composition.resolve",
      "composition.save",
      "composition.export",
    ]);
    assertEquals(plan.cli_commands, [
      "ugoite composition list",
      "ugoite composition inspect <id> [--revision] [--raw]",
      "ugoite composition lint <file>",
      "ugoite composition save <file>",
      "ugoite composition query <id> --param k=v",
      "ugoite composition export <id> --output <path>",
      "ugoite composition import <file>",
    ]);
    assertEquals(plan.browser_golden_journey.model_connection, "disabled");
    assert(plan.browser_golden_journey.steps.length >= 5);

    assertEquals(plan.evidence_record_fields, [
      "source_sha",
      "command",
      "selector",
      "surface",
      "fixture",
      "environment",
      "result",
      "artifact",
      "gap",
    ]);
    assertEquals(
      plan.milestones.map(({ id }) => id),
      ["B0", "B1", "B2", "B3", "B4", "B5", "B6"],
    );

    for (const milestone of plan.milestones) {
      assertEquals(milestone.status, "planned", milestone.id);
      assert(milestone.outcome.trim().length > 0, milestone.id);
      assert(milestone.reserved_selectors.length > 0, milestone.id);
      for (const selector of milestone.reserved_selectors) {
        assert(selector.path.length > 0, milestone.id);
        if (selector.selector.kind === "test") {
          assert(selector.selector.name.length > 0, milestone.id);
        } else {
          assertEquals(selector.selector.kind, "file", milestone.id);
        }
      }
    }

    for (const path of plan.fixture_layout.planned_files) {
      assert(path.startsWith("e2e/fixtures/composition/"), path);
    }
  },
);
