import { afterEach, describe, expect, it } from "vitest";
import { setLocale } from "./i18n";
import {
  decodeSearchHistoryName,
  displaySqlName,
  encodeSearchHistoryName,
  UNTITLED_SQL_NAME,
  type SearchHistoryCriteria,
} from "./sql-metadata";

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
    const name = encodeSearchHistoryName(criteria);

    expect(name).not.toContain("詳細検索");
    expect(decodeSearchHistoryName(name)).toEqual(criteria);
    expect(displaySqlName(name)).toContain("詳細検索");

    setLocale("en");
    expect(displaySqlName(name)).toContain("Advanced search");
  });

  it("localizes generated untitled SQL only at display time", () => {
    expect(displaySqlName(UNTITLED_SQL_NAME)).toBe("Untitled query");
    setLocale("ja");
    expect(displaySqlName(UNTITLED_SQL_NAME)).toBe("無題のクエリ");
  });

  it("leaves operator-provided SQL names unchanged", () => {
    setLocale("ja");
    expect(displaySqlName("My saved query")).toBe("My saved query");
  });
});
