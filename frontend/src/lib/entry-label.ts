import { t } from "~/lib/i18n";
import type { EntryRecord } from "~/lib/types";

/** Prefer a human-authored title/name field; an Entry ID is never a label. */
export const entryDisplayLabel = (
  entry?: Pick<EntryRecord, "id" | "properties">,
): string => {
  if (entry) {
    for (const fieldName of ["title", "name"]) {
      const value = Object.entries(entry.properties ?? {}).find(([key]) =>
        key.trim().toLowerCase() === fieldName
      )?.[1];
      if (typeof value !== "string") continue;
      const label = value.trim();
      if (label && label !== entry.id) return label;
    }
  }
  return t("dashboard.entry");
};
