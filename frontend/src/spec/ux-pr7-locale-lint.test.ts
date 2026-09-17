import { describe, expect, it } from "vitest";
import uiDictionary from "../../../shared/i18n/ui.json";

// Stale keys removed by the UX program. PR-3 row migration orphaned the
// Spaces table-column labels and the long-form entry create label (issue
// #2865, removed in PR-7); PR-5 removed the in-app About dictionary
// (nav.about plus the about.* and aboutPage.* families, removed in PR-5).
// PR-6 orphans (issue #2872) stay live until PR-6 merges and are intentionally
// absent from this list: every entry here must be absent from the shipped
// dictionary at this base.
const STALE_KEY_DENYLIST = [
  "entriesPage.newButton",
  "spacesPage.columnName",
  "spacesPage.columnOpen",
  "spacesPage.columnSettings",
  "nav.about",
];

const STALE_KEY_PREFIXES = ["about.", "aboutPage."];

const checkLocale = (locale: "en" | "ja") => {
  const keys = Object.keys(uiDictionary[locale]);
  for (const stale of STALE_KEY_DENYLIST) {
    expect(
      keys,
      `stale locale key still shipped in ${locale}: ${stale}`,
    ).not.toContain(stale);
  }
  for (const prefix of STALE_KEY_PREFIXES) {
    expect(
      keys.filter((key) => key.startsWith(prefix)),
      `stale locale prefix still shipped in ${locale}: ${prefix}`,
    ).toEqual([]);
  }
};

describe("UX PR-7 locale lint", () => {
  it("REQ-UX-I18N-001: keeps the English and Japanese key sets identical", () => {
    expect(Object.keys(uiDictionary.en).sort()).toEqual(
      Object.keys(uiDictionary.ja).sort(),
    );
  });

  it("REQ-UX-I18N-001: ships no stale dictionary keys orphaned by the UX program", () => {
    checkLocale("en");
    checkLocale("ja");
  });
});
