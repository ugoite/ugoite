/**
 * PR11 (#2422) migration changed-scope rule.
 *
 * Policy (source of truth: AGENTS.md Specification contract;
 * documented in docs/spec/migration.md):
 * - No Ugoite-specific exemption exists in the Mitase validator for
 *   migrated-criterion changed-scope validation, and none may be added.
 * - A migration PR satisfies changed-scope by changing at least one
 *   canonically-owned artifact of that criterion. When the semantic
 *   definition migrated, the canonical spec artifact is in change scope.
 * - Never fake-change a retired legacy implementation/test surface as
 *   "migration": unchanged implementation/test artifacts are acceptable
 *   exactly when a canonical spec artifact changed.
 * - The repository-wide `mitase check .` readiness gate is not weakened
 *   (preset strict, target traceable).
 * - Mitase stays pinned to an immutable 0.1.x release (#2530 stays open;
 *   no HEAD pinning).
 *
 * Representative migrated criterion: REQ-FORM-002#preserve-physical-fields
 * - canonical spec: docs/mitase/requirements/forms.yaml
 * - canonical implementation: crates/ugoite-iceberg/src/form.rs
 *   (FEAT-FORM-001#binding.core/target.upsert-form)
 * - canonical verification: crates/ugoite-iceberg/tests/test_form.rs
 *   (sql_columns_remain_stable_when_pre_v1_renames_are_rejected)
 */

import { assert } from "@std/assert/assert";
import { assertEquals } from "@std/assert/equals";

const CRITERION = "REQ-FORM-002#criterion.preserve-physical-fields";
const CANONICAL_SPEC = "docs/mitase/requirements/forms.yaml";
const CANONICAL_FEATURE = "docs/mitase/features/forms.yaml";
const IMPLEMENTATION = "crates/ugoite-iceberg/src/form.rs";
const VERIFICATION = "crates/ugoite-iceberg/tests/test_form.rs";
const VERIFICATION_TEST =
  "sql_columns_remain_stable_when_pre_v1_renames_are_rejected";
const MIGRATION_LEDGER = "docs/spec/migration.md";

export type MigrationChangeScope = {
  canonicalSpecChanged: boolean;
  implementationChanged: boolean;
  verificationChanged: boolean;
};

/**
 * Repository-owned encoding of the PR11 rule: only a change to a
 * canonically-owned artifact satisfies changed-scope. Implementation or
 * verification churn alone never satisfies it, and there is no exemption
 * parameter by construction (single input object, single boolean output).
 */
export function migrationSatisfiesChangedScope(
  change: MigrationChangeScope,
): boolean {
  return change.canonicalSpecChanged;
}

Deno.test("PR11: representative migrated criterion is canonically owned", async () => {
  const spec = await Deno.readTextFile(CANONICAL_SPEC);
  assert(spec.includes("id: REQ-FORM-002"), "canonical spec owns REQ-FORM-002");
  assert(
    spec.includes("id: preserve-physical-fields"),
    "canonical spec owns preserve-physical-fields",
  );
  assert(
    spec.includes(CRITERION),
    "canonical spec binds the representative criterion",
  );
  assert(
    spec.includes(VERIFICATION_TEST),
    "canonical spec binds the exact verification case",
  );

  const feature = await Deno.readTextFile(CANONICAL_FEATURE);
  assert(
    feature.includes("crates/ugoite-iceberg/src/form.rs"),
    "canonical feature binds the implementation artifact",
  );
  assert(
    feature.includes(CRITERION),
    "canonical feature satisfies the representative criterion",
  );

  for (const path of [CANONICAL_SPEC, CANONICAL_FEATURE, IMPLEMENTATION]) {
    const stat = await Deno.stat(path);
    assert(stat.isFile, `${path} must exist`);
  }
  const verificationSource = await Deno.readTextFile(VERIFICATION);
  assert(
    verificationSource.includes(VERIFICATION_TEST),
    `${VERIFICATION} must contain ${VERIFICATION_TEST}`,
  );
});

Deno.test("PR11: unchanged implementation/tests satisfy iff canonical spec changed", () => {
  // No exemption path: one input object, one boolean output.
  assertEquals(migrationSatisfiesChangedScope.length, 1);

  // Migrated criterion with untouched implementation/test artifacts still
  // satisfies changed-scope when the canonical spec artifact changed.
  assertEquals(
    migrationSatisfiesChangedScope({
      canonicalSpecChanged: true,
      implementationChanged: false,
      verificationChanged: false,
    }),
    true,
  );
  assertEquals(
    migrationSatisfiesChangedScope({
      canonicalSpecChanged: true,
      implementationChanged: true,
      verificationChanged: true,
    }),
    true,
  );

  // Fake-changing only legacy/old implementation/tests as "migration"
  // never satisfies the rule.
  for (
    const change of [
      {
        canonicalSpecChanged: false,
        implementationChanged: false,
        verificationChanged: false,
      },
      {
        canonicalSpecChanged: false,
        implementationChanged: true,
        verificationChanged: false,
      },
      {
        canonicalSpecChanged: false,
        implementationChanged: false,
        verificationChanged: true,
      },
      {
        canonicalSpecChanged: false,
        implementationChanged: true,
        verificationChanged: true,
      },
    ] satisfies MigrationChangeScope[]
  ) {
    assertEquals(
      migrationSatisfiesChangedScope(change),
      false,
      `must not satisfy without a canonical spec change: ${
        JSON.stringify(change)
      }`,
    );
  }
});

Deno.test("PR11: no migrated-criterion exemption exists in validator config", async () => {
  const config = await Deno.readTextFile("mitase.yaml");
  assert(config.includes("preset: strict"), "readiness preset stays strict");
  assert(
    config.includes("target: traceable"),
    "readiness target stays traceable",
  );
  assert(
    config.includes(CRITERION),
    "representative migrated criterion stays a readiness probe",
  );
  assert(
    config.includes("baseline:") && config.includes("require_owned_changes:"),
    "changed baseline contract stays declared",
  );
  for (
    const key of [
      "exempt",
      "exemption",
      "ignore_changed",
      "ignore-changed",
      "skip_changed",
      "skip-changed",
      "allow_unchanged",
      "allow-unchanged",
      "migrated_exemption",
      "migrated-exemption",
    ]
  ) {
    const pattern = new RegExp(`^\\s*${key}\\s*:`, "im");
    assertEquals(
      pattern.test(config),
      false,
      `mitase.yaml must not add a ${key} exemption key`,
    );
  }

  const bootstrap = await Deno.readTextFile("scripts/mitase");
  assertEquals(
    /exempt/i.test(bootstrap),
    false,
    "scripts/mitase must not carry exemption logic",
  );
  assertEquals(
    bootstrap.includes("2422"),
    false,
    "scripts/mitase must not carry issue-specific validator logic",
  );
});

Deno.test("PR11: migration ledger documents the changed-scope rule", async () => {
  const ledger = await Deno.readTextFile(MIGRATION_LEDGER);
  for (
    const snippet of [
      "#2422",
      "canonically-owned artifact",
      "canonical spec artifact",
      "Never fake-change",
      "no Ugoite-specific exemption",
      "mitase check .",
    ]
  ) {
    assert(
      ledger.includes(snippet),
      `migration ledger must document ${JSON.stringify(snippet)}`,
    );
  }
  const agents = await Deno.readTextFile("AGENTS.md");
  assert(
    agents.includes("Preserve source meaning when migrating"),
    "AGENTS.md Specification contract stays the source of truth",
  );
});

Deno.test("PR11: Mitase pin stays on an immutable 0.1.x release", async () => {
  const lock = await Deno.readTextFile("tools/mitase.lock.toml");
  assert(lock.includes('version = "0.1.3"'), "lock stays on Mitase 0.1.3");
  for (
    const target of [
      "x86_64-unknown-linux-gnu",
      "aarch64-unknown-linux-gnu",
      "x86_64-apple-darwin",
      "aarch64-apple-darwin",
    ]
  ) {
    assert(
      lock.includes(`[target.${target}]`),
      `lock keeps target ${target}`,
    );
  }
  assertEquals(/HEAD|main|latest/i.test(lock), false);
});
