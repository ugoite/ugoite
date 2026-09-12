import { validateEntryDraft } from "~/lib/ugoite-client/protocol";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import type { Form } from "~/lib/types";

const FALLBACK_FORM_ID = "00000000-0000-0000-0000-000000000001";
const FALLBACK_REFERENCE_FORM_ID = "00000000-0000-0000-0000-000000000002";

const isUuid = (value: string) =>
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
    value.trim(),
  );

/**
 * Convert a frontend Form to the Rust FormDefinition shape expected by
 * `entry.validate_draft`. This is a mechanical shape adapter only; coercion
 * and validation stay in Rust. Unknown server-only flags default to deny so
 * both sides reject unknown fields the same way.
 */
export const toRustFormDefinition = (form: Form): Record<string, unknown> => {
  const entries = Object.entries(form.fields || {});
  // Legacy frontend "number" maps to the Rust "double" contract; Rust has no
  // bare "number" variant and would otherwise reject the form shape instead
  // of producing field-level diagnostics.
  const toRustFieldType = (type: string) => type === "number" ? "double" : type;
  return {
    id: form.id && isUuid(form.id) ? form.id : FALLBACK_FORM_ID,
    version: form.version ?? 1,
    name: form.name,
    fields: entries.map(([name, field], index) => ({
      id: typeof field.id === "number" && field.id >= 100
        ? field.id
        : 100 + index,
      name,
      field_type: toRustFieldType(field.type),
      required: Boolean(field.required),
      ...(field.deprecated ? { deprecated: true } : {}),
      ...(field.target_form?.trim()
        ? {
          reference_form: isUuid(field.target_form.trim())
            ? field.target_form.trim()
            : FALLBACK_REFERENCE_FORM_ID,
        }
        : {}),
      ...(field.items
        ? {
          items: {
            type: field.items.type,
            ...(field.items.target_form?.trim()
              ? {
                target_form: isUuid(field.items.target_form.trim())
                  ? field.items.target_form.trim()
                  : FALLBACK_REFERENCE_FORM_ID,
              }
              : {}),
          },
        }
        : {}),
    })),
    allow_extra_attributes: false,
  };
};

export type EntryDraftInput = {
  title: string;
  tags: string[];
  fields: Record<string, unknown>;
};

export type EntryDraftValidationSuccess = {
  ok: true;
  normalized: unknown;
};

export type EntryDraftValidationFailure = {
  ok: false;
  code: string;
  message: string;
  invalidFields: string[];
  error: UgoiteApiError;
};

const readWarnings = (detail: unknown): Array<Record<string, unknown>> => {
  if (!detail || typeof detail !== "object" || Array.isArray(detail)) return [];
  const warnings = (detail as { warnings?: unknown }).warnings;
  return Array.isArray(warnings)
    ? warnings.filter((item): item is Record<string, unknown> =>
      Boolean(item) && typeof item === "object" && !Array.isArray(item)
    )
    : [];
};

const readUnknownFields = (detail: unknown): string[] => {
  if (!detail || typeof detail !== "object" || Array.isArray(detail)) return [];
  const fields = (detail as { fields?: unknown }).fields;
  return Array.isArray(fields)
    ? fields.filter((field): field is string => typeof field === "string")
    : [];
};

/** Extract field names from a Rust validation error (warnings + unknown). */
export const invalidFieldsFromError = (error: unknown): string[] => {
  if (!(error instanceof UgoiteApiError)) return [];
  const detail = error.detail as unknown;
  const fields = new Set<string>();
  for (const warning of readWarnings(detail)) {
    if (typeof warning.field === "string" && warning.field) {
      fields.add(warning.field);
    }
  }
  for (const field of readUnknownFields(detail)) fields.add(field);
  return [...fields];
};

/**
 * Validate a draft with the shared Rust boundary. On success returns the
 * normalized value; on failure preserves the canonical `code` so the
 * frontend classification matches the server mutation classification.
 */
export const validateEntryDraftViaWasm = async (
  form: Form,
  draft: EntryDraftInput,
): Promise<EntryDraftValidationSuccess | EntryDraftValidationFailure> => {
  try {
    const normalized = await validateEntryDraft(
      toRustFormDefinition(form),
      {
        title: draft.title,
        form_name: form.name,
        tags: draft.tags,
        fields: draft.fields,
        extra_attributes: {},
      },
    );
    return { ok: true, normalized };
  } catch (error) {
    if (error instanceof UgoiteApiError) {
      return {
        ok: false,
        code: error.code ?? "FORM_VALIDATION_FAILED",
        message: error.message,
        invalidFields: invalidFieldsFromError(error),
        error,
      };
    }
    throw error;
  }
};
