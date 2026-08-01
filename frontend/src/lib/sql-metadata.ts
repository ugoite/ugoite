import type { SqlEntry, SqlMetadata } from "./types";
import { t } from "./i18n";

export type SearchHistoryCriteria = NonNullable<
  SqlMetadata["searchCriteria"]
>;

export const formatSearchHistoryLabel = (
  criteria: SearchHistoryCriteria,
): string => {
  const parts: string[] = [];

  if (criteria.formName) {
    parts.push(t("searchPage.history.form", { value: criteria.formName }));
  }
  for (const tag of criteria.tags) {
    parts.push(t("searchPage.history.tag", { value: tag }));
  }
  if (criteria.updatedFrom) {
    parts.push(
      t("searchPage.history.updatedFrom", { value: criteria.updatedFrom }),
    );
  }
  if (criteria.updatedTo) {
    parts.push(
      t("searchPage.history.updatedTo", { value: criteria.updatedTo }),
    );
  }
  for (const condition of criteria.fieldConditions.slice(0, 2)) {
    const symbol = {
      equals: "=",
      contains: "~",
      lt: "<",
      lte: "<=",
      gt: ">",
      gte: ">=",
    }[condition.operator];
    parts.push(`${condition.field}${symbol}${condition.value}`);
  }

  const extraConditions = criteria.fieldConditions.length - 2;
  if (extraConditions > 0) {
    parts.push(t("searchPage.history.more", { count: extraConditions }));
  }
  if (parts.length === 0) return t("searchPage.advancedSearch");

  const label = `${t("searchPage.advancedSearch")} - ${parts.join(" - ")}`;
  return label.length > 120 ? `${label.slice(0, 117)}...` : label;
};

export const displaySqlName = (
  entry: Pick<SqlEntry, "name" | "kind" | "metadata">,
): string => {
  if (entry.name !== null && entry.name !== "") return entry.name;
  if (entry.kind === "search-history" && entry.metadata?.searchCriteria) {
    return formatSearchHistoryLabel(entry.metadata.searchCriteria);
  }
  return t("sqlPage.untitledQuery");
};
