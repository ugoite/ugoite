/** Mirrors the Rust domain metadata registry (ugoite-domain metadata.rs):
 * SQL, User, UserGroup, and the composition registry form are never
 * user-creatable entry forms. */
export const RESERVED_METADATA_CLASSES = [
  "SQL",
  "User",
  "UserGroup",
  "_ugoite_compositions",
] as const;

const FORM_NAME_PATTERN = /^[A-Za-z0-9_-]+$/;
const FORM_NAME_MAX_BYTES = 128;

export type FormNameValidationIssue = "syntax" | "reserved" | "duplicate";

type NamedForm = { name: string };

const RESERVED_METADATA_CLASS_SET = new Set(
  RESERVED_METADATA_CLASSES.map((name) => name.trim().toLowerCase()),
);

export function isReservedMetadataForm(name: string): boolean {
  return RESERVED_METADATA_CLASS_SET.has(name.trim().toLowerCase());
}

/** Mirrors the safe identifier contract enforced by the Rust domain crate. */
export function isCanonicalFormName(name: string): boolean {
  const value = name.trim();
  return value.length > 0 && value.length <= FORM_NAME_MAX_BYTES &&
    FORM_NAME_PATTERN.test(value);
}

export function getFormNameValidationIssue(
  name: string,
  existingNames: readonly string[],
): FormNameValidationIssue | null {
  const value = name.trim();
  if (!isCanonicalFormName(value)) return "syntax";
  if (isReservedMetadataForm(value)) return "reserved";
  if (existingNames.some((existingName) => existingName === value)) {
    return "duplicate";
  }
  return null;
}

export function filterCreatableEntryForms<T extends NamedForm>(
  forms: readonly T[],
): T[] {
  return forms.filter((form) => !isReservedMetadataForm(form.name));
}
