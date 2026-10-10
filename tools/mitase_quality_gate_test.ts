import { assertEquals } from "@std/assert/equals";
import {
  type BaselineFile,
  compareBaselines,
  isEnforced,
  isQuality,
  type QualityDiagnostic,
  qualityFingerprint,
} from "./mitase_quality_gate.ts";

function diagnostic(
  code: string,
  subject: string,
  reference = "",
  related: string[] = [],
): QualityDiagnostic {
  return {
    code,
    subject: { kind: "spec-anchor", value: subject },
    reference: reference ? { kind: "spec-anchor", value: reference } : null,
    relation: related.length > 0
      ? {
        relation: "governed-by",
        source: { kind: "spec-anchor", value: subject },
        targets: related.map((value) => ({ kind: "spec-anchor", value })),
      }
      : null,
  };
}

function baseline(...fingerprints: string[]): BaselineFile {
  return {
    mitase_version: "0.2.5",
    entries: fingerprints.map((fingerprint) => ({ fingerprint, note: "test" })),
  };
}

Deno.test("quality fingerprints ignore line numbers and statement text", () => {
  const moved = {
    ...diagnostic(
      "MITASE-QUALITY-001",
      "REQ-UX-DASH-001#criterion.recent-count-bound",
      "POL-UI-013#rule.governance",
      ["POL-UI-013#rule.governance"],
    ),
  };
  assertEquals(
    qualityFingerprint(moved),
    "MITASE-QUALITY-001|REQ-UX-DASH-001#criterion.recent-count-bound|POL-UI-013#rule.governance|POL-UI-013#rule.governance",
  );
});

Deno.test("quality gate passes on recorded findings and reports resolved", () => {
  const known = diagnostic(
    "MITASE-QUALITY-001",
    "REQ-A#criterion.x",
    "POL-A#rule.y",
    [
      "POL-A#rule.y",
    ],
  );
  const stale = diagnostic(
    "MITASE-QUALITY-002",
    "POL-B#rule.z",
    "REQ-B#criterion.w",
    [
      "REQ-B#criterion.w",
    ],
  );
  const comparison = compareBaselines(
    [known],
    baseline(qualityFingerprint(known), qualityFingerprint(stale)),
  );
  assertEquals(comparison.added, []);
  assertEquals(comparison.resolved, [qualityFingerprint(stale)]);
  assertEquals(comparison.current, 1);
});

Deno.test("quality gate fails on new enforced findings only", () => {
  const recorded = diagnostic(
    "MITASE-QUALITY-001",
    "REQ-A#criterion.x",
    "POL-A#rule.y",
    [
      "POL-A#rule.y",
    ],
  );
  const fresh = diagnostic(
    "MITASE-QUALITY-004",
    "REQ-C#criterion.m",
    "REQ-D#criterion.n",
    [
      "REQ-D#criterion.n",
    ],
  );
  const info = diagnostic("MITASE-QUALITY-005", "POL-E#rule.governance");
  const comparison = compareBaselines(
    [recorded, fresh, info],
    baseline(qualityFingerprint(recorded)),
  );
  assertEquals(comparison.added, [qualityFingerprint(fresh)]);
  assertEquals(comparison.resolved, []);
});

Deno.test("quality gating ignores non-quality and info codes", () => {
  assertEquals(isQuality({ code: "MITASE-QUALITY-001" }), true);
  assertEquals(isQuality({ code: "MITASE-POLICY-001" }), false);
  assertEquals(isEnforced({ code: "MITASE-QUALITY-004" }), true);
  assertEquals(isEnforced({ code: "MITASE-QUALITY-005" }), false);
});
