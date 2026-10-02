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
    recovery_fixture_candidates: Array<{
      path: string;
      scenario: string;
      status: string;
    }>;
    integration_payloads: {
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
  recovery_and_authorization: {
    status: string;
    selector_binding_status: string;
    save_enablement: {
      enabled: boolean;
      status: string;
      prerequisites: string[];
    };
    cases: Array<{
      id: string;
      surface: string;
      boundary: string;
      expectation: string;
    }>;
  };
  milestones: Milestone[];
};

const PLAN_PATH = "e2e/fixtures/composition/acceptance-plan.json";

const EXPECTED_BROWSER_STEPS = [
  "Save a Composition to the active Space and confirm success only after the Entry receipt identifies the published revision.",
  "Close the browser context, reopen the Space Home, and open the saved Composition.",
  "Change a declared parameter and run the source through the existing paged query path.",
  "Advance a result page and confirm the displayed rows come from the query response without Browser aggregation.",
  "Change a parameter, Space, or source while an earlier request is delayed and confirm stale success, error, finalization, and loading updates are discarded.",
  "Confirm page, scroll, result, continuation, and cache state are absent from the saved Composition.",
];

const EXPECTED_RECOVERY_CASES = [
  {
    id: "concealed_composition_reads",
    surface: "API, CLI, Browser",
    boundary: "Composition-scoped authorized get and history service",
    expectation:
      "Missing and denied Composition reads have the same existing caller-visible generic 404/error shape and do not reveal existence.",
  },
  {
    id: "source_unavailable_projection",
    surface: "Resolver and query",
    boundary:
      "Authorized Composition read followed by current source authorization",
    expectation:
      "After Composition access is authorized, missing and denied sources produce the same source_unavailable diagnostic without source IDs or metadata; field diagnostics require an authorized Form read.",
  },
  {
    id: "current_acl_after_resolve_and_each_page",
    surface: "Query",
    boundary:
      "Existing EntryQuery and exact-revision Saved SQL execution paths",
    expectation:
      "Resolve does not grant access; execution and every continuation page recheck current authorization, including revocation after resolve and during paging.",
  },
  {
    id: "raw_unknown_version_and_broken_reference_recovery",
    surface: "Server, CLI core/remote, Browser",
    boundary: "Composition-scoped exact raw revision reader",
    expectation:
      "Raw inspect, export, and history preserve the exact stored spec and revision metadata for unsupported format versions, malformed documents, and broken references; version is identified before strict v1 typed deserialization.",
  },
  {
    id: "append_only_history_and_restore",
    surface: "API, CLI core/remote, Browser",
    boundary: "Composition-scoped authorized Entry history and restore service",
    expectation:
      "History reads exact revisions; restore publishes a new revision and returns its receipt identity without rewriting prior history.",
  },
  {
    id: "generic_entry_write_guard",
    surface: "Storage and API",
    boundary:
      "Every generic Entry mutation route and Composition-scoped validation",
    expectation:
      "Public generic create, update, bulk, import, and restore cannot bypass Composition validation; reserved-Form generic restore is denied.",
  },
  {
    id: "save_receipt_reconciliation",
    surface: "API, CLI core/remote, Browser",
    boundary:
      "Composition save and existing publication receipt reconciliation",
    expectation:
      "Save reports success only when the receipt identifies the published revision; a lost response remains unknown until the exact revision is reconciled, and retry does not create a second revision.",
  },
  {
    id: "stale_browser_request_state",
    surface: "Browser",
    boundary: "Transient Composition query generation state",
    expectation:
      "Parameter, Space, and source changes suppress stale success, error, finalization, and loading-state updates.",
  },
  {
    id: "portable_surface_contract_parity",
    surface:
      "Rust API client, TypeScript protocol, Server OpenAPI, CLI core/remote",
    boundary: "Shared operation inventory and transport adapters",
    expectation:
      "Operations, prepare/decode behavior, HTTP routes, CLI output meaning, and diagnostic codes agree against the same frozen contracts.",
  },
  {
    id: "exact_candidate_release_evidence",
    surface: "Release verification",
    boundary: "Exact candidate source and artifact manifest",
    expectation:
      "Evidence records source and candidate SHA, artifact digest, actual command, selector, surface, fixture, environment, result, artifact, and gap; planned or unexecuted selectors remain unverified.",
  },
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
    "crates/ugoite-iceberg/tests/test_composition.rs#test:read_raw_composition_preserves_unknown_version_and_malformed_spec",
    "crates/ugoite-iceberg/tests/test_composition.rs#test:read_exact_composition_revision_does_not_fallback_to_latest",
    "crates/ugoite-iceberg/tests/test_composition.rs#test:authorized_composition_raw_read_conceals_missing_and_denied_ids",
  ],
  B3: [
    "crates/ugoite-core/src/composition.rs#test:entry_query_template_compiles_to_bounded_entry_query",
    "crates/ugoite-core/src/composition.rs#test:saved_sql_source_requires_the_exact_revision",
    "crates/ugoite-core/src/composition.rs#test:source_resolution_and_continuation_recheck_current_authorization",
  ],
  B4: [
    "crates/ugoite-api-client/src/lib.rs#test:composition_operations_prepare_and_decode_through_the_portable_protocol",
    "crates/ugoite-server/src/lib.rs#test:composition_handlers_use_authorized_service_boundaries",
    "crates/ugoite-server/src/lib.rs#test:composition_resolve_conceals_missing_and_denied_forms",
    "crates/ugoite-server/src/lib.rs#test:composition_resolve_requires_exact_saved_sql_revision_and_conceals_denial",
    "crates/ugoite-server/src/lib.rs#test:composition_resolve_uses_exact_requested_revision_with_newer_revision_available",
    "crates/ugoite-cli/src/commands/composition.rs#test:composition_core_and_remote_outputs_preserve_diagnostic_codes",
  ],
  B5: [
    "frontend/src/lib/composition-api.ts#test:composition_query_handle_discards_stale_response_state_updates",
    "frontend/src/components/CompositionRenderer.test.tsx#test:composition_renderer_uses_paged_results_without_client_aggregation",
    "e2e/composition-golden-journey.test.ts#test:Browser reopens a saved Composition with model connection disabled",
  ],
  B6: [
    "e2e/composition-golden-journey.test.ts#test:Composition remains portable across Space reopen and query pagination",
    "e2e/composition-cli-parity.test.ts#test:Composition CLI inspect query and export agree in core and remote modes",
    "e2e/composition-recovery-authorization.test.ts#test:Composition raw recovery and ACL denial preserve caller-visible contracts",
    "e2e/composition-save-receipt.test.ts#test:Composition save reconciles a lost response without duplicate publication",
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
    assertEquals(plan.fixture_layout.recovery_fixture_candidates, [
      {
        path: "e2e/fixtures/composition/unknown-format-version.ugcomp.yaml",
        scenario: "unsupported_format_version",
        status: "raw_fixture_only",
      },
      {
        path: "e2e/fixtures/composition/broken-source-reference.ugcomp.yaml",
        scenario: "missing_form",
        status: "raw_fixture_only",
      },
    ]);
    for (const fixture of plan.fixture_layout.recovery_fixture_candidates) {
      assert(
        (await Deno.stat(fixture.path)).isFile,
        `the raw recovery fixture candidate must exist: ${fixture.path}`,
      );
    }
    assertEquals(plan.fixture_layout.integration_payloads.status, "planned");
    assertEquals(plan.fixture_layout.integration_payloads.created, false);
    assertEquals(plan.fixture_layout.integration_payloads.planned_files, [
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
      "composition.history",
      "composition.restore",
    ]);
    assertEquals(plan.cli_commands, [
      "ugoite composition list",
      "ugoite composition inspect <id> [--revision] [--raw]",
      "ugoite composition history <id>",
      "ugoite composition restore <id> --revision <revision>",
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
      "candidate_sha",
      "command",
      "selector",
      "surface",
      "fixture",
      "environment",
      "result",
      "artifact",
      "artifact_digest",
      "gap",
    ]);
    assertEquals(plan.recovery_and_authorization, {
      status: "planned",
      selector_binding_status: "pending_public_storage_and_resolver_contracts",
      save_enablement: {
        enabled: false,
        status:
          "blocked_until_generic_entry_write_guard_and_receipt_reconciliation",
        prerequisites: [
          "Generic Entry create, update, bulk, import, and restore paths cannot bypass Composition validation.",
          "Generic restore of the reserved Composition Form is denied while Composition-scoped restore validates.",
          "A successful save is confirmed by the canonical receipt identifying the published revision.",
          "A lost save response remains outcome-unknown until receipt reconciliation confirms the exact revision; retry does not publish a duplicate.",
        ],
      },
      cases: EXPECTED_RECOVERY_CASES,
    });
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

    for (const path of plan.fixture_layout.integration_payloads.planned_files) {
      assert(path.startsWith("e2e/fixtures/composition/"), path);
    }
  },
);
