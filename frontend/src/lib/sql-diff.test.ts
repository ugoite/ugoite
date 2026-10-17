import { describe, expect, it } from "vitest";
import { buildSqlLineDiff } from "./sql-diff";

describe("buildSqlLineDiff", () => {
  it("keeps a compact two-line context around a small SQL change", () => {
    const currentLines = Array.from(
      { length: 100 },
      (_, index) => `line ${index}`,
    );
    const generatedLines = [...currentLines];
    currentLines[50] = "current value";
    generatedLines[50] = "generated value";

    const diff = buildSqlLineDiff(
      currentLines.join("\n"),
      generatedLines.join("\n"),
    );

    expect(diff).toContainEqual({ kind: "removed", line: "current value" });
    expect(diff).toContainEqual({ kind: "added", line: "generated value" });
    expect(
      diff.filter((line) => line.kind === "context").map((line) => line.line),
    )
      .toEqual(["line 48", "line 49", "line 51", "line 52"]);
    expect(diff.filter((line) => line.kind === "omitted")).toEqual([
      { kind: "omitted", line: "", omittedCount: 48 },
      { kind: "omitted", line: "", omittedCount: 47 },
    ]);
  });

  it("keeps separate distant hunks when the input exceeds the matrix bound", () => {
    const currentLines = Array.from(
      { length: 600 },
      (_, index) => `line ${index}`,
    );
    const generatedLines = [...currentLines];
    currentLines[100] = "current first value";
    generatedLines[100] = "generated first value";
    currentLines[500] = "current second value";
    generatedLines[500] = "generated second value";

    const diff = buildSqlLineDiff(
      currentLines.join("\n"),
      generatedLines.join("\n"),
    );

    expect(
      diff.filter((line) => line.kind === "removed").map((line) => line.line),
    )
      .toEqual(["current first value", "current second value"]);
    expect(
      diff.filter((line) => line.kind === "added").map((line) => line.line),
    )
      .toEqual(["generated first value", "generated second value"]);
    expect(
      diff.filter((line) => line.kind === "context").map((line) => line.line),
    )
      .toEqual([
        "line 98",
        "line 99",
        "line 101",
        "line 102",
        "line 498",
        "line 499",
        "line 501",
        "line 502",
      ]);
    expect(diff.filter((line) => line.kind === "omitted")).toEqual([
      { kind: "omitted", line: "", omittedCount: 98 },
      { kind: "omitted", line: "", omittedCount: 395 },
      { kind: "omitted", line: "", omittedCount: 97 },
    ]);
  });

  it("shows an unchanged marker for metadata-only SQL proposals", () => {
    expect(buildSqlLineDiff("SELECT $filter_value", "SELECT $filter_value"))
      .toEqual([{ kind: "unchanged", line: "" }]);
  });

  it("uses a preview-limit marker before splitting a high-line-count query", () => {
    const diff = buildSqlLineDiff("\n".repeat(129_999), "SELECT 1");

    expect(diff).toEqual([{ kind: "preview-limit", line: "" }]);
  });

  it("does not claim exact line counts for a high-distance fallback with matching interior lines", () => {
    const currentLines = Array.from(
      { length: 600 },
      (_, index) => `line ${index}`,
    );
    const generatedLines = [...currentLines];
    for (let index = 10; index <= 530; index += 4) {
      currentLines[index] = `current ${index}`;
      generatedLines[index] = `generated ${index}`;
    }

    const diff = buildSqlLineDiff(
      currentLines.join("\n"),
      generatedLines.join("\n"),
    );

    expect(diff).toContainEqual({ kind: "omitted-range", line: "" });
    expect(diff.some((line) => line.kind === "omitted-changes")).toBe(false);
    expect(
      diff.some((line) => line.kind === "removed" || line.kind === "added"),
    )
      .toBe(false);
  });

  it("declines to split SQL that exceeds the character preview bound", () => {
    expect(buildSqlLineDiff("x".repeat(256 * 1024 + 1), "SELECT 1"))
      .toEqual([{ kind: "preview-limit", line: "" }]);
  });

  it("declines to split SQL that exceeds the line preview bound", () => {
    expect(buildSqlLineDiff("\n".repeat(10_000), "SELECT 1"))
      .toEqual([{ kind: "preview-limit", line: "" }]);
  });
});
