import { afterEach, describe, expect, it } from "vitest";
import { setLocale } from "./i18n";
import { displaySqlName, type SearchHistoryCriteria } from "./sql-metadata";

const criteria: SearchHistoryCriteria = {
  formName: "Meeting",
  tags: ["project"],
  updatedFrom: "2026-01-01",
  updatedTo: "2026-01-03",
  fieldConditions: [{ field: "Status", operator: "equals", value: "Active" }],
};

describe("sql metadata", () => {
  afterEach(() => setLocale("en"));

  it("stores search history as locale-neutral structured metadata", () => {
    setLocale("ja");
    const entry = {
      name: null,
      kind: "search-history" as const,
      metadata: { searchCriteria: criteria },
    };

    expect(JSON.stringify(entry)).not.toContain("ugoite.search-history");
    expect(displaySqlName(entry)).toContain("詳細検索");

    setLocale("en");
    expect(displaySqlName(entry)).toContain("Advanced search");
  });

  it("localizes generated untitled SQL only at display time", () => {
    const entry = {
      name: null,
      kind: "user-query" as const,
      metadata: { generatedName: "untitled" as const },
    };
    expect(displaySqlName(entry)).toBe("Untitled query");
    setLocale("ja");
    expect(displaySqlName(entry)).toBe("無題のクエリ");
  });

  it("leaves operator-provided SQL names unchanged", () => {
    setLocale("ja");
    expect(displaySqlName({
      name: "My saved query",
      kind: "user-query",
    })).toBe("My saved query");
  });
});
