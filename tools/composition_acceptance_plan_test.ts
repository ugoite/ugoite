import { assert, assertEquals } from "@std/assert";

type ReservedSelector = {
  path: string;
  selector: { kind: "test"; name: string } | { kind: "file" };
};

type Milestone = {
  id: string;
  outcome: string;
  reserved_selectors: ReservedSelector[];
};

type AcceptancePlan = {
  schema: string;
  implementation_contract_frozen: boolean;
  fixture_layout: {
    root: string;
    shared_document_fixtures: Array<{
      path: string;
      owner: string;
    }>;
    recovery_fixture_candidates: Array<{
      path: string;
      scenario: string;
    }>;
    integration_payloads: {
      created_files: string[];
    };
  };
  operation_inventory: string[];
  cli_commands: string[];
  browser_golden_journey: {
    model_connection: string;
    steps: string[];
  };
  reactive_variable_studio_journey: {
    model_connection: string;
    selector: { path: string; kind: "test"; name: string };
    steps: string[];
  };
  reactive_variable_metric_journey: {
    model_connection: string;
    selector: { path: string; kind: "test"; name: string };
    steps: string[];
  };
  reactive_variable_latest_generation_journey: {
    model_connection: string;
    selector: { path: string; kind: "test"; name: string };
    steps: string[];
  };
  evidence_record_fields: string[];
  recovery_and_authorization: {
    save_enablement: {
      scope: string;
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

type EvidenceArchive = {
  schema: string;
  note: string;
  runtime_evidence: Array<Record<string, string>>;
};

const PLAN_PATH = "e2e/fixtures/composition/acceptance-plan.json";
const ARCHIVE_PATH = "e2e/fixtures/composition/evidence-archive.json";

const EXPECTED_BROWSER_STEPS = [
  "Open a parameterized Saved SQL result, declare typed Composition Variables, bind the SQL variable names explicitly, and place labeled Input components before saving the tool through Studio.",
  "Delay and interrupt the first save response after commit, then retry with the same Idempotency-Key and verify one history publication and the original receipt revision.",
  "Close the browser context, rediscover the saved tool from Space Home, and open the exact revision named by the receipt.",
  "Use the labeled Inputs to change declared Variable values, advance a result page, and confirm the displayed rows come from the existing query response without Browser aggregation.",
  "Change a Variable or Space, switch a source, or navigate through Composition History to another exact revision while a request is delayed; confirm stale success, error, finalization, and loading updates are discarded.",
  "Confirm page, scroll, result, continuation, and cache state are absent from the saved Composition.",
];

const EXPECTED_REACTIVE_VARIABLE_STUDIO_STEPS = [
  "In Studio, declare a string variable with a default, bind an Input and Text to it, and bind date variables to the pinned Saved SQL revision before saving once.",
  "Read the exact saved revision and verify its pinned Saved SQL revision and the SQL-name-to-variable bindings.",
  "Reopen the exact saved revision and verify its declared defaults and variable-backed Text are restored.",
  "Change only the string Input and verify Text updates in the same UI cycle without reissuing either the Saved SQL Table or Composition Metric query that already ran.",
  "Reload the exact saved revision and verify the declared default returns while Composition history remains at one revision.",
];

const EXPECTED_REACTIVE_VARIABLE_METRIC_STEPS = [
  "Seed one Space Form with Amount values 10, 20, and 30, then create a normal Saved SQL query that returns COUNT(*) and SUM(Amount) for Amount >= the typed minimum variable.",
  "In Studio, declare an integer variable with default 0, bind the exact Saved SQL revision, place a labeled Input and two Metrics for the count and sum columns, and save once.",
  "Read the exact Composition revision and verify it pins the Saved SQL revision used by both Metric displays.",
  "Rediscover and reopen the saved tool from Space Home; minimum 0 displays count 3 and sum 60.",
  "Change the Input to minimum 20 and verify the authorized server SQL result updates the Metrics to count 2 and sum 50.",
  "Reload the exact Composition revision; the declared default 0 and values 3 / 60 return while history remains at one revision.",
];

const EXPECTED_REACTIVE_VARIABLE_LATEST_GENERATION_STEPS = [
  "Seed Amount values 1, 2, and 3, then create a saved Composition whose typed minimum variable drives one Saved SQL Source used by both Metric and Table.",
  "Edit the required minimum Input through 1, 2, and 3 while preserving each prior Metric and Table response unresolved; the test removes transport abort signals so response-generation checks are exercised independently of cancellation.",
  "Resolve the responses in reverse order, 3 then 2 then 1, and verify only minimum 3 remains visible in both Metric and Table with no alert or stale state change.",
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
      "Variable, Space, source, and exact Composition revision changes suppress stale success, error, finalization, and loading-state updates.",
  },
  {
    id: "revoked_source_read_clears_browser_outputs",
    surface: "Browser and Server",
    boundary:
      "Viewer-visible exact Composition revision with independently controlled Form and Saved SQL Read policies",
    expectation:
      "After either Form Read or Saved SQL Read is revoked while Composition access remains, Metric and Table clear prior data before reload and replaying the formerly authorized SQL request cannot return data. Form Read revocation produces a generic execution error while Saved SQL Read revocation returns source_unavailable. Form identity is concealed when Form Read is revoked; Saved SQL identity and details are concealed when Saved SQL Read is revoked. SQL text and protected result text remain concealed in both cases.",
  },
  {
    id: "composition_runtime_results_are_space_isolated",
    surface: "Browser and Server",
    boundary:
      "One live browser session switching between Space-owned exact Composition revisions",
    expectation:
      "Identical variable, source, and component identifiers with identical typed inputs show only the current Space's authorized Metric and Table results.",
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
    "crates/ugoite-cli/src/commands/composition.rs#test:list_reads_bounded_local_pages_without_loading_specs",
    "crates/ugoite-cli/src/commands/composition.rs#test:list_remote_request_decodes_and_returns_the_portable_page",
    "crates/ugoite-cli/src/commands/composition.rs#test:history_reads_bounded_local_pages_with_raw_revision_data",
    "crates/ugoite-cli/src/commands/composition.rs#test:history_prepares_a_bounded_page_request_through_the_portable_protocol",
    "crates/ugoite-cli/src/commands/composition.rs#test:history_remote_request_decodes_and_returns_the_portable_page",
    "crates/ugoite-cli/src/main.rs#test:composition_history_accepts_bounded_page_options_without_becoming_a_write",
  ],
  B5: [
    "frontend/src/lib/composition-api.ts#test:composition_query_handle_discards_stale_response_state_updates",
    "frontend/src/components/CompositionRenderer.test.tsx#test:composition_renderer_delegates_to_source_native_presenters_without_client_aggregation",
    "e2e/composition-golden-journey.test.ts#test:Browser saves a tool once, reopens its exact revision from Home, and pages parameterized results with model connection disabled",
    "e2e/composition-table-metric-pagination.test.ts#test:Table pagination leaves the SQL Metric and typed Input unchanged",
    "e2e/composition-revision-stale-response.test.ts#test:discard a delayed metric response after navigating to another exact revision",
  ],
  B6: [
    "e2e/composition-golden-journey.test.ts#test:Browser saves a tool once, reopens its exact revision from Home, and pages parameterized results with model connection disabled",
    "crates/ugoite-cli/tests/test_journey_remote.rs#test:test_cli_composition_inspect_query_export_match_core_remote",
    "e2e/composition-recovery-authorization.test.ts#test:Composition raw recovery and ACL denial preserve caller-visible contracts",
    "e2e/composition-save-receipt.test.ts#test:Composition save reconciles a lost response without duplicate publication",
    "docs/architecture/testing/composition-acceptance.md#file",
  ],
  B7: [
    "e2e/composition-source-acl-isolation.test.ts#test:Viewer Metric and Table clear after Form or Saved SQL Read is revoked",
    "e2e/composition-source-acl-isolation.test.ts#test:Space switching shows only the selected Metric result",
  ],
};

function selectorDetails({ path, selector }: ReservedSelector): string {
  return selector.kind === "test"
    ? `${path}#${selector.kind}:${selector.name}`
    : `${path}#${selector.kind}`;
}

Deno.test(
  "Composition acceptance plan records selectors and evidence shape without claiming release status",
  async () => {
    const raw = JSON.parse(
      await Deno.readTextFile(PLAN_PATH),
    ) as Record<string, unknown>;
    const plan = raw as AcceptancePlan;

    assertEquals(plan.schema, "ugoite/composition-acceptance-plan/v1");
    // The gate file carries no plan status: status-like fields would infer
    // current state from history. Implementation status stays in Mitase.
    for (
      const key of [
        "status",
        "runtime_evidence_recorded",
        "runtime_evidence",
      ] as const
    ) {
      assert(
        !(key in raw),
        `the acceptance plan must not carry ${key}`,
      );
    }
    assertEquals(plan.implementation_contract_frozen, true);
    assertEquals(plan.fixture_layout.root, "e2e/fixtures/composition");
    const sharedFixtures = plan.fixture_layout.shared_document_fixtures;
    assertEquals(sharedFixtures, [
      {
        path:
          "crates/ugoite-domain/tests/fixtures/composition/sql-first.ugcomp.yaml",
        owner: "ugoite-domain",
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
      },
      {
        path: "e2e/fixtures/composition/broken-source-reference.ugcomp.yaml",
        scenario: "missing_form",
      },
    ]);
    for (const fixture of plan.fixture_layout.recovery_fixture_candidates) {
      assert(
        (await Deno.stat(fixture.path)).isFile,
        `the raw recovery fixture candidate must exist: ${fixture.path}`,
      );
    }
    assertEquals(plan.fixture_layout.integration_payloads.created_files, [
      "e2e/fixtures/composition/space-seed/manifest.json",
    ]);
    for (const path of plan.fixture_layout.integration_payloads.created_files) {
      assert(
        (await Deno.stat(path)).isFile,
        `the Composition integration payload must exist: ${path}`,
      );
    }
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
      "composition.preview",
      "composition.save",
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
      "ugoite composition query <id> --var k=v",
      "ugoite composition export <id> --output <path>",
      "ugoite composition import <file>",
    ]);
    assertEquals(plan.browser_golden_journey.model_connection, "disabled");
    assertEquals(plan.browser_golden_journey.steps, EXPECTED_BROWSER_STEPS);
    assertEquals(
      plan.reactive_variable_studio_journey.model_connection,
      "disabled",
    );
    assertEquals(plan.reactive_variable_studio_journey.selector, {
      path: "e2e/composition-studio-journey.test.ts",
      kind: "test",
      name: "Composition Studio builds a reusable tool from existing knowledge",
    });
    assertEquals(
      plan.reactive_variable_studio_journey.steps,
      EXPECTED_REACTIVE_VARIABLE_STUDIO_STEPS,
    );
    assertEquals(
      plan.reactive_variable_metric_journey.model_connection,
      "disabled",
    );
    assertEquals(plan.reactive_variable_metric_journey.selector, {
      path: "e2e/composition-studio-journey.test.ts",
      kind: "test",
      name: "Composition Studio saves and reopens a filtered SQL metric",
    });
    assertEquals(
      plan.reactive_variable_metric_journey.steps,
      EXPECTED_REACTIVE_VARIABLE_METRIC_STEPS,
    );
    assertEquals(
      plan.reactive_variable_latest_generation_journey.model_connection,
      "disabled",
    );
    assertEquals(plan.reactive_variable_latest_generation_journey.selector, {
      path: "e2e/composition-latest-variable-generation.test.ts",
      kind: "test",
      name:
        "Rapid typed-variable edits keep only the latest Metric and Table result",
    });
    assertEquals(
      plan.reactive_variable_latest_generation_journey.steps,
      EXPECTED_REACTIVE_VARIABLE_LATEST_GENERATION_STEPS,
    );

    const seedManifest = JSON.parse(
      await Deno.readTextFile(
        "e2e/fixtures/composition/space-seed/manifest.json",
      ),
    ) as { selectors: string[] };
    assertEquals(seedManifest.selectors, [
      EXPECTED_RESERVED_SELECTORS.B5[2],
    ]);
    assertEquals(
      EXPECTED_RESERVED_SELECTORS.B5[2],
      EXPECTED_RESERVED_SELECTORS.B6[0],
    );

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
    assertEquals(plan.recovery_and_authorization.save_enablement, {
      scope: "cross_surface_recovery_and_receipt_acceptance",
      prerequisites: [
        "Generic Entry create, update, bulk, import, and restore paths cannot bypass Composition validation.",
        "Generic restore of the reserved Composition Form is denied while Composition-scoped restore validates.",
        "A successful save is confirmed by the canonical receipt identifying the published revision.",
        "A lost save response remains outcome-unknown until receipt reconciliation confirms the exact revision; retry does not publish a duplicate.",
      ],
    });
    assertEquals(
      plan.recovery_and_authorization.cases,
      EXPECTED_RECOVERY_CASES,
    );
    assertEquals(
      plan.milestones.map(({ id }) => id),
      ["B0", "B1", "B2", "B3", "B4", "B5", "B6", "B7"],
    );

    for (const milestone of plan.milestones) {
      assert(
        !("status" in (milestone as Record<string, unknown>)),
        `${milestone.id} must not carry a static status`,
      );
      assert(milestone.outcome.trim().length > 0, milestone.id);
      assertEquals(
        milestone.reserved_selectors.map(selectorDetails),
        EXPECTED_RESERVED_SELECTORS[milestone.id],
        `${milestone.id} reserved acceptance selectors changed`,
      );
    }

    for (const path of plan.fixture_layout.integration_payloads.created_files) {
      assert(path.startsWith("e2e/fixtures/composition/"), path);
    }
  },
);

Deno.test(
  "Composition evidence archive preserves historical runtime evidence shape",
  async () => {
    const archive = JSON.parse(
      await Deno.readTextFile(ARCHIVE_PATH),
    ) as EvidenceArchive;
    const plan = JSON.parse(
      await Deno.readTextFile(PLAN_PATH),
    ) as AcceptancePlan;

    assertEquals(archive.schema, "ugoite/composition-evidence-archive/v1");
    assert(archive.note.trim().length > 0);
    assert(archive.runtime_evidence.length > 0);

    for (const record of archive.runtime_evidence) {
      for (const field of plan.evidence_record_fields) {
        const value = record[field];
        assert(
          typeof value === "string" && value.trim().length > 0,
          `archived evidence record is missing evidence field ${field}`,
        );
      }
      assert(
        (await Deno.stat(record.artifact)).isFile,
        "the archived runtime evidence artifact must be present",
      );
      const artifactBytes = await Deno.readFile(record.artifact);
      const artifactDigest = await crypto.subtle.digest(
        "SHA-256",
        artifactBytes,
      );
      const artifactDigestHex = Array.from(
        new Uint8Array(artifactDigest),
        (byte) => byte.toString(16).padStart(2, "0"),
      ).join("");
      assertEquals(artifactDigestHex, record.artifact_digest);
    }
  },
);
