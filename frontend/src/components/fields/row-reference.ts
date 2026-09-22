/**
 * Shared row-reference helpers.
 *
 * Both the create dialog and the Entry detail editor resolve reference
 * options through this module so label display, stable-ID storage, and
 * target-Form scoping stay identical. Validity authority stays in Rust;
 * nothing here decides saveability.
 *
 * Existing-value display is a point read: the stored stable entry id is
 * resolved through an authorized Entry get, the returned Entry's Form is
 * checked against the target Form, and the human Preview is derived from
 * the Entry's structured fields. The candidate list page position never
 * influences the displayed label, so references outside the first page
 * still show their actual Preview. Wrong-Form, deleted, and unauthorized
 * references resolve to safe unavailable states; raw entry ids are never
 * used as human-facing labels.
 */

export interface RowReferenceOption {
  id: string;
  title: string;
  label: string;
}

export const rowReferenceSuggestionLimit = 8;

export const normalizeRowReferenceTargetForm = (def: {
  target_form?: string;
}) => def.target_form?.trim() ?? "";

export const hasRowReferencePicker = (
  def: { type: string; target_form?: string },
  spaceId: string,
) =>
  Boolean(spaceId.trim()) &&
  def.type === "row_reference" &&
  normalizeRowReferenceTargetForm(def) !== "";

/** Backend `entry_preview` budget (crates/ugoite-iceberg/src/service.rs). */
export const rowReferencePreviewCharLimit = 512;

export interface RowReferencePreviewForm {
  id?: string;
  name: string;
  fields: Record<string, { type: string }>;
}

/**
 * Render one structured value the way the backend preview does:
 * strings verbatim, scalars via `to_string`, collections as JSON.
 */
export const displayableRowReferenceValue = (value: unknown): string => {
  if (typeof value === "string") return value;
  if (typeof value === "boolean" || typeof value === "number") {
    return String(value);
  }
  if (Array.isArray(value)) {
    try {
      return JSON.stringify(value);
    } catch {
      return "";
    }
  }
  if (value !== null && typeof value === "object") {
    try {
      return JSON.stringify(value);
    } catch {
      return "";
    }
  }
  return "";
};

/**
 * Derive the human Preview for a point-read Entry, mirroring the backend
 * `entry_preview`: target-Form field order, `name: value` parts joined
 * with " · ", truncated to the backend char budget. Reads the structured
 * fields from the Entry frontmatter object; non-object frontmatter yields
 * an empty Preview (callers fall back to a safe generic label, never the
 * raw entry id).
 */
export const buildRowReferencePreview = (
  frontmatter: unknown,
  form: RowReferencePreviewForm,
): string => {
  if (frontmatter === null || typeof frontmatter !== "object") return "";
  const values = frontmatter as Record<string, unknown>;
  const parts: string[] = [];
  for (const name of Object.keys(form.fields ?? {})) {
    const value = values[name];
    if (value === null || value === undefined) continue;
    const rendered = displayableRowReferenceValue(value);
    if (rendered === "") continue;
    parts.push(`${name}: ${rendered}`);
  }
  const preview = parts.join(" · ");
  return Array.from(preview).slice(0, rowReferencePreviewCharLimit).join("");
};

/**
 * Check a point-read Entry's Form against the picker target. The stored
 * Entry Form is a human-readable name; accept either the target name or
 * the target stable id so fixtures carrying either shape keep working.
 */
export const rowReferenceTargetMatches = (
  entryForm: unknown,
  target: RowReferencePreviewForm,
): boolean => {
  if (typeof entryForm !== "string" || entryForm.trim() === "") return false;
  const actual = entryForm.trim();
  return actual === target.name || (target.id ?? "") === actual;
};

export const buildRowReferenceOptions = (
  entries: Array<{ id: string }>,
): RowReferenceOption[] =>
  entries
    .map((entry) => {
      return {
        id: entry.id,
        title: entry.id,
        label: entry.id,
      };
    })
    .sort(
      (left, right) => left.id.localeCompare(right.id),
    );
