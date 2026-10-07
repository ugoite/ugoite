import type {
  EntryFieldCapability,
  EntryFilter,
  EntryFilterOperator,
  EntrySort,
} from "./entry-query";
import type {
  EntryQueryCompositionFieldSchemaEntry,
  EntryQueryCompositionFilter,
  EntryQueryCompositionSort,
} from "./entry-query-composition";
import type { Form } from "./types";
import { formApi } from "./ugoite-client";

/**
 * Studio EntryQuery capability source. The draft Knowledge carries only
 * field IDs and types (`EntryQueryCompositionFieldSchemaEntry`); human
 * names and filter/sort capabilities come from the live Form definition,
 * loaded transiently and never stored. Every helper degrades to the
 * schema snapshot when the definition is unavailable so editing never
 * blocks.
 */

/** Operators offered when a field carries no backend capability metadata. */
const fallbackOperators: EntryFilterOperator[] = [
  "equals",
  "contains",
  "lt",
  "lte",
  "gt",
  "gte",
];

/** Extract a `{parameter}` binding name, if the value is exactly one. */
export const studioBindingName = (value: unknown): string | undefined => {
  if (value !== null && typeof value === "object") {
    const record = value as Record<string, unknown>;
    if (
      typeof record.parameter === "string" && record.parameter.length > 0
    ) {
      return record.parameter;
    }
  }
  return undefined;
};

/** Display text for a parameter binding; the shared dialog edits strings. */
export const studioBindingText = (name: string): string => `{{${name}}}`;

/**
 * Round-trip `{{name}}` display text back to a parameter binding so a
 * dialog apply never silently flattens a binding to a plain string.
 */
export const studioParseBindingText = (text: string): unknown => {
  const binding = /^\{\{([^}]+)\}\}$/.exec(text);
  if (binding && binding[1].length > 0) return { parameter: binding[1] };
  return text;
};

/** Human name per field ID from the transient Form definition. */
export const studioFieldNames = (
  form: Form | undefined,
): Map<number, string> => {
  const names = new Map<number, string>();
  if (!form) return names;
  for (const [key, definition] of Object.entries(form.fields ?? {})) {
    const id = definition.query_capability?.field.field_id ?? definition.id;
    if (typeof id !== "number") continue;
    if (!names.has(id)) names.set(id, definition.query_capability?.name ?? key);
  }
  return names;
};

/**
 * Dialog capabilities from the transient Form definition. Fields without
 * backend capability metadata keep their record-key name with the full
 * operator set, matching the previous blind-pick behavior for legacy forms.
 */
export const studioCapabilitiesFromForm = (
  form: Form,
): EntryFieldCapability[] => {
  const capabilities: EntryFieldCapability[] = [];
  for (const [key, definition] of Object.entries(form.fields ?? {})) {
    const id = definition.query_capability?.field.field_id ?? definition.id;
    if (typeof id !== "number") continue;
    const capability = definition.query_capability;
    capabilities.push({
      field: { kind: "property", field_id: id },
      name: capability?.name ?? key,
      field_type: capability?.field_type ?? definition.type,
      filterable: capability?.filterable ?? true,
      sortable: capability?.sortable ?? true,
      projectable: capability?.projectable ?? true,
      supported_operators: capability
        ? [...capability.supported_operators]
        : [...fallbackOperators],
    });
  }
  return capabilities;
};

/**
 * Fallback capabilities from the draft schema snapshot. Names fall back to
 * the current field-ID rendering; every schema field stays offered so a
 * missing definition never blocks editing.
 */
export const studioFallbackCapabilities = (
  schema: readonly EntryQueryCompositionFieldSchemaEntry[],
): EntryFieldCapability[] =>
  schema.map((entry) => ({
    field: { kind: "property", field_id: entry.field_id },
    name: String(entry.field_id),
    field_type: entry.field_type,
    filterable: true,
    sortable: true,
    projectable: true,
    supported_operators: [...fallbackOperators],
  }));

/** Draft filter to the shared dialog shape; bindings become `{{name}}`. */
export const studioFilterToEntryFilter = (
  filter: EntryQueryCompositionFilter,
): EntryFilter => {
  const binding = studioBindingName(filter.value);
  return {
    field: { kind: "property", field_id: filter.field_id },
    operator: filter.operator,
    value: binding !== undefined ? studioBindingText(binding) : filter.value,
  };
};

/**
 * Shared dialog filter back to the draft shape. Non-property refs have no
 * draft grammar and fail closed with `undefined`; `{{name}}` text becomes
 * a parameter binding again.
 */
export const studioEntryFilterToComposition = (
  filter: EntryFilter,
): EntryQueryCompositionFilter | undefined => {
  if (filter.field.kind !== "property") return undefined;
  const raw = typeof filter.value === "string"
    ? studioParseBindingText(filter.value)
    : filter.value;
  return {
    field_id: filter.field.field_id,
    operator: filter.operator,
    value: raw ?? "",
  };
};

/** Draft sort to the shared dialog shape. */
export const studioSortToEntrySort = (
  clause: EntryQueryCompositionSort,
): EntrySort => ({
  field: { kind: "property", field_id: clause.field_id },
  direction: clause.direction,
});

/** Shared dialog sort back to the draft shape; non-property refs fail closed. */
export const studioEntrySortToComposition = (
  item: EntrySort,
): EntryQueryCompositionSort | undefined =>
  item.field.kind !== "property" ? undefined : {
    field_id: item.field.field_id,
    direction: item.direction,
  };

const formDefinitionCache = new Map<string, Form>();

/** Clear the transient per-source Form cache (tests only). */
export const clearStudioFormDefinitionCache = (): void => {
  formDefinitionCache.clear();
};

/**
 * Load the referenced Form definition transiently, cached per source.
 * Returns `undefined` when the form is absent; transport failures reject
 * so the editor can show an explicit error while degrading to the schema
 * snapshot. Nothing here is stored in the draft.
 */
export const fetchStudioFormDefinition = async (
  spaceId: string,
  formId: string,
): Promise<Form | undefined> => {
  const key = `${spaceId}/${formId}`;
  const cached = formDefinitionCache.get(key);
  if (cached) return cached;
  const forms = await formApi.list(spaceId);
  for (const form of forms) {
    if (form.id) formDefinitionCache.set(`${spaceId}/${form.id}`, form);
  }
  return formDefinitionCache.get(key);
};
