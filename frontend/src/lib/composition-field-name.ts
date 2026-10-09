import type { Form, FormField } from "./types";

/** Display-only Form field name shared by Composition authoring and rendering. */
export const compositionFormFieldName = (
  forms: readonly Form[] | undefined,
  formId: string,
  fieldId: number,
): string | undefined => {
  const fields = (forms ?? []).find((form) => form.id === formId)?.fields ?? {};
  const entry = Object.entries(fields).find(([, field]) =>
    (field.query_capability?.field.field_id ?? field.id) === fieldId
  );
  if (!entry) return undefined;

  const [key, field] = entry as [string, FormField];
  const label = field.label?.trim();
  if (label) return label;
  const capabilityName = field.query_capability?.name.trim();
  return capabilityName || key;
};
