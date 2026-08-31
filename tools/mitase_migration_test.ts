import { assertEquals } from "@std/assert/equals";
import { parse } from "yaml";

type RecordValue = Record<string, unknown>;
type LegacyPolicy = {
  id: string;
  linked_requirements: string[];
  linked_specifications: string[];
};

const legacyPolicyPath = "docs/spec/policies/policies.yaml";
const legacyFoundationPath = "docs/spec/philosophy/foundation.yaml";
const canonicalPolicyPath = "docs/mitase/policies/policies.yaml";
const canonicalFoundationPath = "docs/mitase/philosophies/foundation.yaml";
const policyEdgesPath = "docs/mitase-migration/policy-edges.yaml";
const policyLevelsPath = "docs/mitase-migration/policy-levels.yaml";

function asRecord(value: unknown, subject: string): RecordValue {
  assertEquals(typeof value, "object", `${subject} must be an object`);
  assertEquals(value !== null, true, `${subject} must not be null`);
  return value as RecordValue;
}

function asRecords(value: unknown, subject: string): RecordValue[] {
  assertEquals(Array.isArray(value), true, `${subject} must be an array`);
  return value as RecordValue[];
}

function asStrings(value: unknown, subject: string): string[] {
  assertEquals(Array.isArray(value), true, `${subject} must be an array`);
  const values = value as unknown[];
  assertEquals(
    values.every((item) => typeof item === "string"),
    true,
    `${subject} must contain strings`,
  );
  return values as string[];
}

function valueString(value: RecordValue, key: string, subject: string): string {
  assertEquals(
    typeof value[key],
    "string",
    `${subject}.${key} must be a string`,
  );
  return value[key] as string;
}

async function sha256(path: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    await Deno.readFile(path),
  );
  return [...new Uint8Array(digest)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function canonicalRules(
  document: RecordValue,
): Map<string, { anchor: string; level: string; governedBy: string[] }> {
  const result = new Map<
    string,
    { anchor: string; level: string; governedBy: string[] }
  >();
  for (const policy of asRecords(document.policies, "canonical policies")) {
    const policyId = valueString(policy, "id", "canonical policy");
    const rules = asRecords(policy.rules, `${policyId}.rules`);
    assertEquals(rules.length, 1, `${policyId} must have one governance rule`);
    const rule = rules[0];
    const ruleId = valueString(rule, "id", `${policyId}.rule`);
    result.set(policyId, {
      anchor: `${policyId}#rule.${ruleId}`,
      level: valueString(rule, "level", `${policyId}.rule`),
      governedBy: asStrings(rule.governed_by, `${policyId}.rule.governed_by`),
    });
  }
  return result;
}

Deno.test("migrated governance has one guarded authority", async () => {
  const legacy = asRecord(
    parse(await Deno.readTextFile(legacyPolicyPath)),
    "legacy policies",
  );
  const canonical = asRecord(
    parse(await Deno.readTextFile(canonicalPolicyPath)),
    "canonical policies",
  );
  const levels = asRecord(
    parse(await Deno.readTextFile(policyLevelsPath)),
    "policy level decisions",
  );

  const legacyPolicies = asRecords(
    legacy.policies,
    "legacy policies",
  ) as LegacyPolicy[];
  const canonicalPolicies = asRecords(canonical.policies, "canonical policies");
  const canonicalIds = canonicalPolicies.map((policy) =>
    valueString(policy, "id", "canonical policy")
  );
  assertEquals(canonicalIds[0], "POL-000");
  assertEquals(
    canonicalIds.slice(1),
    legacyPolicies.map((policy) => policy.id),
  );

  const decisions = asRecords(levels.decisions, "policy level decisions");
  assertEquals(
    decisions.map((decision) => valueString(decision, "policy", "decision")),
    canonicalIds,
  );
  const decisionLevels = new Map(
    decisions.map((decision) => [
      valueString(decision, "policy", "decision"),
      valueString(decision, "level", "decision"),
    ]),
  );
  const rules = canonicalRules(canonical);
  assertEquals(rules.size, canonicalIds.length);

  for (const policy of canonicalPolicies) {
    const policyId = valueString(policy, "id", "canonical policy");
    const rule = rules.get(policyId);
    assertEquals(rule !== undefined, true, `${policyId} rule must be indexed`);
    assertEquals(
      decisionLevels.get(policyId),
      rule?.level,
      `${policyId} level must match its explicit decision`,
    );
    assertEquals(
      rule?.governedBy.includes("PHIL-000#principle.stable-foundation"),
      true,
      `${policyId} must remain below Foundation precedence`,
    );

    if (rule?.level !== "must") continue;
    const bindingClaims = asRecords(policy.bindings, `${policyId}.bindings`)
      .flatMap((binding) =>
        asRecords(binding.targets, `${policyId}.binding.targets`)
      )
      .flatMap((target) =>
        asRecords(target.claims, `${policyId}.target.claims`)
      );
    assertEquals(
      bindingClaims.some((claim) =>
        (claim.kind === "enforces" && claim.rule === rule.anchor) ||
        (claim.kind === "evidences" && claim.anchor === rule.anchor)
      ),
      true,
      `${policyId} must have an exact Artifact Binding`,
    );
  }

  const foundation = asRecord(
    parse(await Deno.readTextFile(canonicalFoundationPath)),
    "canonical foundation",
  );
  const philosophies = asRecords(
    foundation.philosophies,
    "canonical philosophies",
  );
  const foundationRoot = philosophies.find((philosophy) =>
    philosophy.id === "PHIL-000"
  );
  assertEquals(foundationRoot !== undefined, true, "PHIL-000 must exist");
  assertEquals(
    asRecords(foundationRoot?.principles, "PHIL-000 principles").map((
      principle,
    ) => valueString(principle, "id", "PHIL-000 principle")),
    [
      "user-owned-knowledge",
      "disposable-work",
      "tools-without-authority",
      "stable-foundation",
    ],
  );
});

Deno.test("legacy policy traceability is preserved as typed deferred edges", async () => {
  const legacy = asRecord(
    parse(await Deno.readTextFile(legacyPolicyPath)),
    "legacy policies",
  );
  const specificationRegistry = asRecords(
    parse(await Deno.readTextFile("docs/spec/specifications.yaml")),
    "legacy specifications",
  );
  const manifest = asRecord(
    parse(await Deno.readTextFile(policyEdgesPath)),
    "policy edge manifest",
  );
  assertEquals(manifest.schema, "ugoite/migration/v1");
  assertEquals(manifest.kind, "policy-traceability");
  const source = asRecord(manifest.source, "policy edge source");
  assertEquals(source.legacy_policy_registry, legacyPolicyPath);
  assertEquals(
    source.legacy_specification_registry,
    "docs/spec/specifications.yaml",
  );
  assertEquals(source.revision, "a872f4992bcb3633681eb0383e101453f00b32db");

  const legacyPolicies = asRecords(
    legacy.policies,
    "legacy policies",
  ) as LegacyPolicy[];
  const specificationEntries = specificationRegistry;
  const knownSpecifications = new Set(
    specificationEntries.map((entry) =>
      valueString(entry, "id", "specification entry")
    ),
  );
  const edges = asRecords(manifest.policy_edges, "policy edges");
  assertEquals(edges.length, legacyPolicies.length);

  for (const legacyPolicy of legacyPolicies) {
    const edge = edges.find((candidate) =>
      candidate.legacy_policy === legacyPolicy.id
    );
    assertEquals(
      edge !== undefined,
      true,
      `${legacyPolicy.id} must have preserved edges`,
    );
    assertEquals(edge?.canonical_rule, `${legacyPolicy.id}#rule.governance`);
    assertEquals(edge?.state, "deferred");
    const relations = asRecords(edge?.edges, `${legacyPolicy.id}.edges`);
    for (
      const [field, targetKind, expected] of [
        [
          "linked_requirements",
          "external-requirement-category",
          legacyPolicy.linked_requirements,
        ],
        [
          "linked_specifications",
          "external-specification",
          legacyPolicy.linked_specifications,
        ],
      ] as const
    ) {
      const relation = relations.find((candidate) => candidate.field === field);
      assertEquals(
        relation !== undefined,
        true,
        `${legacyPolicy.id} must preserve ${field}`,
      );
      assertEquals(relation?.relation, "governs");
      assertEquals(relation?.target_kind, targetKind);
      assertEquals(relation?.target_ids, expected);
      if (field === "linked_specifications") {
        for (const specification of expected) {
          assertEquals(
            knownSpecifications.has(specification),
            true,
            `${specification} must remain in the external specification registry`,
          );
        }
      }
    }
  }
});

Deno.test("legacy migrated snapshots and indexes remain guarded", async () => {
  assertEquals(
    await sha256(legacyFoundationPath),
    "7643c6fd477de0d8a772ac4b47323b55fa5ab2a782549cfa59beccf733eafd13",
  );
  assertEquals(
    await sha256(legacyPolicyPath),
    "411c9a18d02cafb8476d183595dd444100ec091a2993b6b22d1a397f2e747679",
  );

  const index = await Deno.readTextFile("docs/spec/index.md");
  assertEquals(
    index.includes("docs/mitase/philosophies/foundation.yaml"),
    true,
  );
  assertEquals(index.includes("docs/mitase/policies/policies.yaml"), true);
  assertEquals(index.includes("docs/spec/philosophy/foundation.yaml"), false);
  const policyIndex = await Deno.readTextFile("docs/spec/policies/index.md");
  assertEquals(
    policyIndex.includes("docs/mitase-migration/policy-edges.yaml"),
    true,
  );
  const config = await Deno.readTextFile("mitase.yaml");
  for (
    const path of [
      policyEdgesPath,
      policyLevelsPath,
      "tools/mitase_migration_test.ts",
      "tools/knowledge_compatibility.ts",
    ]
  ) {
    assertEquals(
      config.includes(`- ${path}`),
      true,
      `mitase.yaml must declare ${path}`,
    );
  }
});
