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
    shared_document_fixtures: Array<{
      path: string;
      owner: string;
      status: string;
    }>;
    e2e_payloads: {
      status: string;
      created: boolean;
      planned_files: string[];
    };
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

const EXPECTED_BROWSER_STEPS = [
  "Save a Composition to the active Space and confirm success only after the Entry receipt identifies the published revision.",
  "Close the browser context, reopen the Space Home, and open the saved Composition.",
  "Change a declared parameter and run the source through the existing paged query path.",
  "Advance a result page and confirm the displayed rows come from the query response without Browser aggregation.",
  "Change a parameter while an earlier request is delayed and confirm its stale response does not replace the current result.",
  "Confirm page, scroll, result, continuation, and cache state are absent from the saved Composition.",
];

const EXPECTED_RESERVED_SELECTORS: Record<string, string[]> = {
  B0: [
    "crates/ugoite-domain/src/composition.rs#test:restricted_yaml_rejects_unsupported_syntax_with_stable_diagnostics",
    "crates/ugoite-iceberg/tests/test_composition.rs#test:composition_registry_round_trip_is_one_space_entry",
    "crates/ugoite-core/src/composition.rs#test:composition_resolves_through_existing_query_contracts",
  ],
  B1: [
    "crates/ugoite-domain/src/composition.rs#test:composition_canonicalization_is_semantic_and_deterministic",
    "crates/ugoite-wasm/src/lib.rs#test:composition_wasm_matches_native_canonical_output_and_diagnostics",
  ],
  B2: [
    "crates/ugoite-iceberg/tests/test_composition.rs#test:composition_save_get_list_history_restore_and_conflict_use_entry_revisions",
    "crates/ugoite-iceberg/tests/test_composition.rs#test:unknown_version_and_broken_reference_remain_raw_inspectable",
    "crates/ugoite-iceberg/tests/test_composition.rs#test:generic_entry_write_cannot_bypass_composition_validation",
  ],
  B3: [
    "crates/ugoite-core/src/composition.rs#test:entry_query_template_compiles_to_bounded_entry_query",
    "crates/ugoite-core/src/composition.rs#test:saved_sql_source_requires_the_exact_revision",
    "crates/ugoite-core/src/composition.rs#test:source_resolution_rechecks_current_authorization",
  ],
  B4: [
    "crates/ugoite-api-client/src/lib.rs#test:composition_operations_prepare_and_decode_through_the_portable_protocol",
    "crates/ugoite-server/src/lib.rs#test:composition_handlers_use_authorized_service_boundaries",
    "crates/ugoite-cli/src/commands/composition.rs#test:composition_core_and_remote_outputs_preserve_diagnostic_codes",
  ],
  B5: [
    "frontend/src/lib/composition-api.ts#test:composition_query_handle_discards_stale_generations",
    "frontend/src/components/CompositionRenderer.test.tsx#test:composition_renderer_uses_paged_results_without_client_aggregation",
    "e2e/composition-golden-journey.test.ts#test:Browser reopens a saved Composition with model connection disabled",
  ],
  B6: [
    "e2e/composition-golden-journey.test.ts#test:Composition remains portable across Space reopen and query pagination",
    "e2e/composition-cli-parity.test.ts#test:Composition CLI inspect query and export agree in core and remote modes",
    "docs/architecture/testing/composition-acceptance.md#file",
  ],
};

function selectorDetails({ path, selector }: ReservedSelector): string {
  return selector.kind === "test"
    ? `${path}#${selector.kind}:${selector.name}`
    : `${path}#${selector.kind}`;
}

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
    const sharedFixtures = plan.fixture_layout.shared_document_fixtures;
    assertEquals(sharedFixtures, [
      {
        path:
          "crates/ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml",
        owner: "ugoite-domain",
        status: "shared_reference",
      },
    ]);
    assert(
      (await Deno.stat(sharedFixtures[0].path)).isFile,
      "the shared canonical Composition fixture must exist as a file",
    );
    assertEquals(plan.fixture_layout.e2e_payloads.status, "planned");
    assertEquals(plan.fixture_layout.e2e_payloads.created, false);
    assertEquals(plan.fixture_layout.e2e_payloads.planned_files, [
      "e2e/fixtures/composition/unknown-format-version.ugcomp.yaml",
      "e2e/fixtures/composition/broken-source-reference.ugcomp.yaml",
      "e2e/fixtures/composition/space-seed/manifest.json",
      "e2e/fixtures/composition/expected/",
    ]);
    assert(
      !plan.fixture_layout.shared_document_fixtures[0].path.startsWith(
        `${plan.fixture_layout.root}/`,
      ),
      "the shared domain fixture must not be duplicated under the E2E fixture root",
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
    assertEquals(plan.browser_golden_journey.steps, EXPECTED_BROWSER_STEPS);

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
      assertEquals(
        milestone.reserved_selectors.map(selectorDetails),
        EXPECTED_RESERVED_SELECTORS[milestone.id],
        `${milestone.id} reserved acceptance selectors changed`,
      );
    }

    for (const path of plan.fixture_layout.e2e_payloads.planned_files) {
      assert(path.startsWith("e2e/fixtures/composition/"), path);
    }
  },
);
