import { validateEntryDraft } from "~/lib/ugoite-client/protocol";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import type { Form, FormExtraAttributesPolicy } from "~/lib/types";

const EXTRA_ATTRIBUTES_POLICY_METADATA = "ugoite.extra_attributes_policy";

const isUuid = (value: string) =>
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
    value.trim(),
  );

const formIdentityError = (
  message: string,
  detail: Record<string, unknown>,
): UgoiteApiError =>
  new UgoiteApiError({
    kind: "invalid_arguments",
    operation: "entry.validate_draft",
    code: "INVALID_INPUT",
    message,
    detail: { kind: "form_identity", ...detail },
  });

const requireFormId = (form: Form): string => {
  if (!form.id || !isUuid(form.id)) {
    throw formIdentityError(
      `Form '${form.name}' is missing its stable FormId`,
      { form: form.name },
    );
  }
  return form.id;
};

const requireFieldId = (form: Form, name: string, id: number | undefined) => {
  if (!Number.isInteger(id) || id < 100) {
    throw formIdentityError(
      `Field '${name}' in Form '${form.name}' is missing its stable FieldId`,
      { form: form.name, field: name },
    );
  }
  return id;
};

const resolveReferenceFormId = (
  form: Form,
  fieldName: string,
  reference: string,
  knownForms: readonly Form[],
): string => {
  const target = reference.trim();
  // A server-read opaque FormId is already resolved. Human-readable names
  // need the loaded catalog so this adapter never fabricates a target UUID.
  if (isUuid(target)) return target;
  const candidate = knownForms.find((known) =>
    known.name === target || known.id === target
  );
  if (!candidate?.id || !isUuid(candidate.id)) {
    throw formIdentityError(
      `Reference target '${target}' for field '${fieldName}' in Form '${form.name}' could not be resolved`,
      { form: form.name, field: fieldName, target_form: target },
    );
  }
  return candidate.id;
};

/**
 * Convert a frontend Form to the Rust FormDefinition shape expected by
 * `entry.validate_draft`. This is a mechanical shape adapter only; coercion
 * and validation stay in Rust. FormId, FieldId, and reference FormId are
 * loaded durable identities: an incomplete fixture or unresolved name is a
 * typed admission failure, never a synthetic identity.
 */
export const toRustFormDefinition = (
  form: Form,
  knownForms: readonly Form[] = [],
): Record<string, unknown> => {
  const entries = Object.entries(form.fields || {});
  const policy: FormExtraAttributesPolicy = form.allow_extra_attributes ??
    "deny";
  // Legacy frontend "number" maps to the Rust "double" contract; Rust has no
  // bare "number" variant and would otherwise reject the form shape instead
  // of producing field-level diagnostics.
  const toRustFieldType = (type: string) => type === "number" ? "double" : type;
  const formId = requireFormId(form);
  const forms = [form, ...knownForms.filter((known) => known !== form)];
  return {
    id: formId,
    version: form.version ?? 1,
    name: form.name,
    fields: entries.map(([name, field]) => ({
      id: requireFieldId(form, name, field.id),
      name,
      field_type: toRustFieldType(field.type),
      required: Boolean(field.required),
      ...(field.deprecated ? { deprecated: true } : {}),
      ...(field.target_form?.trim()
        ? {
          reference_form: resolveReferenceFormId(
            form,
            name,
            field.target_form,
            forms,
          ),
        }
        : {}),
      ...(field.items
        ? {
          items: {
            type: field.items.type,
            ...(field.items.target_form?.trim()
              ? {
                target_form: resolveReferenceFormId(
                  form,
                  name,
                  field.items.target_form,
                  forms,
                ),
              }
              : {}),
          },
        }
        : {}),
    })),
    // Rust's domain validator currently projects both allowing policies onto
    // its boolean validation flag. Keep the canonical policy alongside that
    // projection so the WASM boundary does not discard the durable meaning.
    allow_extra_attributes: policy !== "deny",
    extension_metadata: {
      [EXTRA_ATTRIBUTES_POLICY_METADATA]: policy,
    },
  };
};

export type EntryDraftInput = {
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
  knownForms: readonly Form[] = [],
): Promise<EntryDraftValidationSuccess | EntryDraftValidationFailure> => {
  try {
    const normalized = await validateEntryDraft(
      toRustFormDefinition(form, knownForms),
      {
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
