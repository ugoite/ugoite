/**
 * Shared row-reference helpers.
 *
 * Both the create dialog and the Entry detail editor resolve reference
 * options through this module so title display, stable-ID storage, and
 * target-Form scoping stay identical. Validity authority stays in Rust;
 * nothing here decides saveability.
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

export const buildRowReferenceOptions = (
  entries: Array<{ id: string; title?: string | null }>,
): RowReferenceOption[] =>
  entries
    .map((entry) => {
      const title = entry.title?.trim() || entry.id;
      return {
        id: entry.id,
        title,
        label: title === entry.id ? entry.id : `${title} (${entry.id})`,
      };
    })
    .sort(
      (left, right) =>
        left.title.localeCompare(right.title) ||
        left.id.localeCompare(right.id),
    );
