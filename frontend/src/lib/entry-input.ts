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
  const offset = `${sign}${pad(Math.floor(absoluteOffset / 60))}:${
    pad(
      absoluteOffset % 60,
    )
  }`;
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

/**
 * Build a structured fields map for the additive `{ form, title, fields }`
 * payload. Values stay strings here; the shared Rust boundary owns coercion
 * and validation. `__*` control keys and blank values are dropped, matching
 * the Markdown builder. Zoned timestamps get the same browser-offset
 * normalization as the Markdown path so `datetime-local` controls round-trip.
 */
export const buildStructuredEntryFields = (
  formDef: Form,
  values: Record<string, string>,
): Record<string, string> => {
  const fields: Record<string, string> = {};
  for (const [name, value] of Object.entries(values)) {
    if (name.startsWith("__")) continue;
    if (!value.trim()) continue;
    const field = formDef.fields?.[name];
    fields[name] = field
      ? normalizeEntryFieldValue(field, value.trim())
      : value.trim();
  }
  return fields;
};

/**
 * Read `tags:` from Markdown frontmatter. Returns `null` when frontmatter
 * carries no `tags` key so callers can keep stored tags (legacy rule);
 * an explicit empty list clears them.
 */
export const parseMarkdownFrontmatterTags = (
  markdown: string,
): string[] | null => {
  const match = /^---\s*\n([\s\S]*?)\n---\s*\n/.exec(markdown);
  if (!match) return null;
  const frontmatter = match[1];
  // Single-line `tags: foo` or inline list.
  const single = /^\s*tags\s*:\s*(.+?)\s*$/.exec(
    frontmatter.split("\n").find((line) => /^\s*tags\s*:/.test(line)) ?? "",
  );
  if (!single) {
    // Multi-line `tags:\n  - a\n  - b`.
    const lines = frontmatter.split("\n");
    const start = lines.findIndex((line) => /^\s*tags\s*:\s*$/.test(line));
    if (start === -1) return null;
    const tags: string[] = [];
    for (const line of lines.slice(start + 1)) {
      const item = /^\s*-\s+(.+?)\s*$/.exec(line);
      if (!item) break;
      tags.push(item[1]);
    }
    return tags;
  }
  const raw = single[1].trim();
  if (raw.startsWith("[")) {
    try {
      const parsed: unknown = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        return parsed.filter((tag): tag is string => typeof tag === "string");
      }
    } catch {
      return [];
    }
    return [];
  }
  return raw ? [raw] : [];
};

/**
 * Parse a compatibility Markdown source back into a structured draft.
 * Source view is ingress only; the returned draft becomes the single
 * authority and preview derives from it.
 */
export const parseMarkdownToStructuredDraft = (
  markdown: string,
): { title: string; fields: Record<string, string> } => {
  const titleMatch = /^#\s+(.+)$/m.exec(markdown);
  const title = titleMatch ? titleMatch[1].trim() : "";
  // Reuse the same H2 parsing as the fields view so source and fields agree.
  // Dynamic import would cycle; keep the regex local and minimal.
  const fields: Record<string, string> = {};
  const lines = markdown.split(/\r?\n/);
  let active: string | null = null;
  let buffer: string[] = [];
  const push = () => {
    if (!active) return;
    if (buffer[buffer.length - 1] === "") buffer = buffer.slice(0, -1);
    fields[active] = buffer.join("\n");
  };
  for (const line of lines) {
    const heading = /^##\s+(.+?)\s*$/.exec(line);
    if (heading) {
      push();
      active = heading[1];
      buffer = [];
      continue;
    }
    if (line.startsWith("#")) {
      push();
      active = null;
      buffer = [];
      continue;
    }
    if (active) buffer.push(line);
  }
  push();
  return { title, fields };
};
