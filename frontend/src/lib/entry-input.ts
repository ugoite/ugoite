import {
  ensureFormFrontmatter,
  replaceFirstH1,
  updateH2Section,
} from "~/lib/markdown";
import type { Form } from "~/lib/types";

const ZONED_TIMESTAMP_TYPES = new Set(["timestamp_tz", "timestamp_tz_ns"]);
const LOCAL_DATETIME_PATTERN =
  /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2})(?::(\d{2})(\.\d+)?)?$/;

const pad = (value: number) => String(value).padStart(2, "0");

const addBrowserTimezoneOffset = (value: string): string => {
  const match = LOCAL_DATETIME_PATTERN.exec(value);
  if (!match) return value;

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  const offsetMinutes = -date.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? "+" : "-";
  const absoluteOffset = Math.abs(offsetMinutes);
  const offset = `${sign}${pad(Math.floor(absoluteOffset / 60))}:${pad(
    absoluteOffset % 60,
  )}`;
  const seconds = match[2] ?? "00";
  const fraction = match[3] ?? "";
  return `${match[1]}:${seconds}${fraction}${offset}`;
};

export const normalizeEntryFieldValue = (
  field: Form["fields"][string],
  value: string,
): string => {
  if (!ZONED_TIMESTAMP_TYPES.has(field.type)) return value;
  return addBrowserTimezoneOffset(value);
};

export type EntryInputMode = "markdown" | "webform" | "chat";

export const buildEntryMarkdownFromFields = (
  formDef: Form,
  title: string,
  fieldValues: Record<string, string>,
): string => {
  let content = ensureFormFrontmatter(
    replaceFirstH1(formDef.template, title),
    formDef.name,
  );
  for (const [name, value] of Object.entries(fieldValues)) {
    if (name.startsWith("__") || !value.trim()) continue;
    const field = formDef.fields?.[name];
    const normalizedValue = field
      ? normalizeEntryFieldValue(field, value.trim())
      : value.trim();
    content = updateH2Section(content, name, normalizedValue);
  }
  return content;
};

export const buildEntryMarkdownByMode = (
  formDef: Form,
  title: string,
  values: Record<string, string>,
  mode: EntryInputMode,
): string => {
  if (mode === "markdown") {
    const originalMarkdown = values.__markdown;
    const trimmedMarkdown = originalMarkdown?.trim();
    if (trimmedMarkdown) return originalMarkdown as string;
  }
  return buildEntryMarkdownFromFields(formDef, title, values);
};
