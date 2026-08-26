import { describe, expect, it } from "vitest";
import { t } from "~/lib/i18n";
import { settingsSections } from "~/lib/settings-sections";

describe("Space settings coverage", () => {
  it("keeps every settings section and the language control addressable", () => {
    expect(settingsSections.map((section) => section.id)).toEqual([
      "general",
      "members",
      "credentials",
      "storage",
      "audit",
    ]);
    expect(t("settings.language")).toBe("Language");
  });
});
