import { t } from "./i18n";

export type SearchHistoryCriteria = {
  formName: string;
  tags: string[];
  updatedFrom: string;
  updatedTo: string;
  fieldConditions: Array<{
    field: string;
    operator: "equals" | "contains";
    value: string;
  }>;
};

type SearchHistoryMetadata = {
  kind: "search-history";
  version: 1;
  criteria: SearchHistoryCriteria;
};

const SEARCH_HISTORY_PREFIX = "ugoite.search-history.v1:";

/** The durable marker for a new SQL entry without a user-provided name. */
export const UNTITLED_SQL_NAME = "ugoite.sql.untitled.v1";

export const encodeSearchHistoryName = (
  criteria: SearchHistoryCriteria,
): string =>
  `${SEARCH_HISTORY_PREFIX}${JSON.stringify({
    kind: "search-history",
    version: 1,
    criteria,
  } satisfies SearchHistoryMetadata)}`;

const isStringArray = (value: unknown): value is string[] =>
  Array.isArray(value) && value.every((item) => typeof item === "string");

const parseCriteria = (value: unknown): SearchHistoryCriteria | null => {
  if (!value || typeof value !== "object") return null;
  const criteria = value as Record<string, unknown>;
  if (
    typeof criteria.formName !== "string" ||
    !isStringArray(criteria.tags) ||
    typeof criteria.updatedFrom !== "string" ||
    typeof criteria.updatedTo !== "string" ||
    !Array.isArray(criteria.fieldConditions)
  ) {
    return null;
  }

  const fieldConditions = criteria.fieldConditions.filter((condition) => {
    if (!condition || typeof condition !== "object") return false;
    const item = condition as Record<string, unknown>;
    return typeof item.field === "string" &&
      (item.operator === "equals" || item.operator === "contains") &&
      typeof item.value === "string";
  }).map((condition) => {
    const item = condition as Record<string, string>;
    return {
      field: item.field,
      operator: item.operator,
      value: item.value,
    };
  });

  if (fieldConditions.length !== criteria.fieldConditions.length) return null;
  return {
    formName: criteria.formName,
    tags: criteria.tags,
    updatedFrom: criteria.updatedFrom,
    updatedTo: criteria.updatedTo,
    fieldConditions,
  };
};

export const decodeSearchHistoryName = (
  name: string,
): SearchHistoryCriteria | null => {
  if (!name.startsWith(SEARCH_HISTORY_PREFIX)) return null;
  try {
    const parsed = JSON.parse(
      name.slice(SEARCH_HISTORY_PREFIX.length),
    ) as Partial<SearchHistoryMetadata>;
    if (parsed.kind !== "search-history" || parsed.version !== 1) return null;
    return parseCriteria(parsed.criteria);
  } catch {
    return null;
  }
};

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
    const symbol = condition.operator === "contains" ? "~" : "=";
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

export const displaySqlName = (name: string): string => {
  if (name === UNTITLED_SQL_NAME) return t("sqlPage.untitledQuery");
  const criteria = decodeSearchHistoryName(name);
  return criteria ? formatSearchHistoryLabel(criteria) : name;
};
