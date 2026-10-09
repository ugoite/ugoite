import type {
  EntryFilterOperator,
  EntryProjection,
  EntryQuery,
  EntrySortDirection,
} from "./entry-query";
import type { Form } from "./types";

/**
 * Pure EntryQuery → Composition EntryQuery-source builder.
 *
 * Fail-closed: anything without an exact Composition grammar (All-scope
 * queries, system field refs, non-scalar values, unmapped field types,
 * unresolvable reference targets) reports `inexpressible` with an i18n
 * reason key instead of approximating.
 */

export type EntryQueryCompositionReason =
  | "entryQueryToolSave.needsForm"
  | "entryQueryToolSave.unsupportedQuery";

/** Canonical Composition FieldType wire names (ugoite-domain serde names). */
const compositionFieldTypes = new Set([
  "string",
  "markdown",
  "sql",
  "boolean",
  "integer",
  "long",
  "float",
  "double",
  "date",
  "time",
  "timestamp",
  "timestamp_tz",
  "timestamp_ns",
  "timestamp_tz_ns",
  "uuid",
  "binary",
  "list",
  "object_list",
  "row_reference",
  "asset_reference",
]);

const compositionOperators: ReadonlySet<EntryFilterOperator> = new Set([
  "equals",
  "contains",
  "lt",
  "lte",
  "gt",
  "gte",
]);

const compositionDirections: ReadonlySet<EntrySortDirection> = new Set([
  "asc",
  "desc",
]);

export interface EntryQueryCompositionListItem {
  type: string;
  target_form?: string;
}

export interface EntryQueryCompositionFieldSchemaEntry {
  field_id: number;
  field_type: string;
  reference_form?: string;
  items?: EntryQueryCompositionListItem;
}

/** Mirrors `MAX_COMPOSITION_COLLECTION_ITEMS` in `ugoite-domain`. */
export const MAX_COMPOSITION_FIELD_SCHEMA_ITEMS = 256;

export interface EntryQueryCompositionFilter {
  field_id: number;
  operator: EntryFilterOperator;
  value: unknown;
}

export interface EntryQueryCompositionSort {
  field_id: number;
  direction: EntrySortDirection;
}

export type EntryQueryCompositionProjection =
  | { kind: "preview" }
  | { kind: "fields"; fields: number[] };

export interface EntryQueryCompositionSource {
  id: string;
  kind: "entry_query";
  form_id: string;
  query: {
    text?: string;
    filters: EntryQueryCompositionFilter[];
    sort: EntryQueryCompositionSort[];
    projection: EntryQueryCompositionProjection;
  };
}

export type EntryQueryCompositionResult =
  | {
    status: "ok";
    source: EntryQueryCompositionSource;
    fieldSchema: EntryQueryCompositionFieldSchemaEntry[];
    warnings: [];
  }
  | { status: "inexpressible"; reason: EntryQueryCompositionReason };

export interface EntryQueryCompositionInput {
  query: EntryQuery;
  projection: EntryProjection;
  form?: Form;
  knownForms?: readonly Form[];
}

const isUuid = (value: string): boolean =>
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
    value.trim(),
  );

const isScalarLiteral = (value: unknown): boolean =>
  value === null ||
  typeof value === "string" ||
  typeof value === "boolean" ||
  (typeof value === "number" && Number.isFinite(value));

const findFormField = (form: Form, fieldId: number) =>
  Object.values(form.fields ?? {}).find((field) =>
    (field.query_capability?.field.field_id ?? field.id) === fieldId
  );

const formFieldId = (field: Form["fields"][string]): number | undefined => {
  const id = field.query_capability?.field.field_id ?? field.id;
  return typeof id === "number" ? id : undefined;
};

const formFieldType = (field: Form["fields"][string]): string =>
  field.query_capability?.field_type ?? field.type;

/**
 * Mirror of `entry_query_text_searches_field_type` in ugoite-core: text
 * search covers every field type except these four. The resolver derives the
 * text-search field set from the live Form, so the snapshot must carry the
 * same set or resolve reports `source_schema_changed`.
 */
const textSearchesFieldType = (fieldType: string): boolean =>
  fieldType !== "list" &&
  fieldType !== "object_list" &&
  fieldType !== "asset_reference" &&
  fieldType !== "binary";

const resolveReferenceFormId = (
  reference: string,
  form: Form,
  knownForms: readonly Form[],
): string | undefined => {
  const target = reference.trim();
  if (!target) return undefined;
  // A server-read opaque FormId is already resolved. Human-readable names
  // need the loaded catalog; an unresolvable name fails closed.
  if (isUuid(target)) return target;
  const candidate = [form, ...knownForms].find((known) =>
    known.name === target || known.id === target
  );
  return candidate?.id && isUuid(candidate.id) ? candidate.id : undefined;
};

const schemaEntryForField = (
  form: Form,
  knownForms: readonly Form[],
  fieldId: number,
): EntryQueryCompositionFieldSchemaEntry | undefined => {
  const field = findFormField(form, fieldId);
  if (!field) return undefined;
  const fieldType = field.query_capability?.field_type ?? field.type;
  if (!compositionFieldTypes.has(fieldType)) return undefined;
  const targetForm = field.target_form?.trim() ?? "";
  if (fieldType === "row_reference") {
    if (field.items) return undefined;
    const referenceForm = resolveReferenceFormId(
      targetForm,
      form,
      knownForms,
    );
    if (!referenceForm) return undefined;
    return {
      field_id: fieldId,
      field_type: fieldType,
      reference_form: referenceForm,
    };
  }
  if (fieldType === "list") {
    // A top-level reference target on a list field has no Composition
    // grammar; only the item schema may carry one.
    if (targetForm) return undefined;
    if (!field.items) return { field_id: fieldId, field_type: fieldType };
    const itemType = field.items.type;
    if (!compositionFieldTypes.has(itemType)) return undefined;
    const itemTarget = field.items.target_form?.trim() ?? "";
    if (itemType === "row_reference") {
      const referenceForm = resolveReferenceFormId(
        itemTarget,
        form,
        knownForms,
      );
      if (!referenceForm) return undefined;
      return {
        field_id: fieldId,
        field_type: fieldType,
        items: { type: itemType, target_form: referenceForm },
      };
    }
    if (itemTarget) return undefined;
    return {
      field_id: fieldId,
      field_type: fieldType,
      items: { type: itemType },
    };
  }
  if (targetForm || field.items) return undefined;
  return { field_id: fieldId, field_type: fieldType };
};

/**
 * Snapshot every current Form field that has an exact Composition schema.
 * Query projections still have their independent 64-field limit; keeping the
 * broader schema lets the editor choose other supported fields for projection,
 * filtering, or sorting without creating references the resolver cannot use.
 */
export const buildEntryQueryCompositionFieldSchema = (
  form: Form,
  knownForms: readonly Form[] = [],
): EntryQueryCompositionFieldSchemaEntry[] => {
  const fieldIds = [
    ...new Set(
      Object.values(form.fields ?? {}).flatMap((field) => {
        const fieldId = formFieldId(field);
        return fieldId === undefined ? [] : [fieldId];
      }),
    ),
  ].sort((left, right) => left - right);
  return fieldIds.flatMap((fieldId) => {
    const entry = schemaEntryForField(form, knownForms, fieldId);
    return entry ? [entry] : [];
  });
};

/**
 * Build the Composition EntryQuery source fragment for the current
 * controller state. Form scope only: the Composition grammar requires a
 * stable `form_id` and property field IDs.
 */
export const buildEntryQueryComposition = (
  input: EntryQueryCompositionInput,
): EntryQueryCompositionResult => {
  const unsupported = (
    reason: EntryQueryCompositionReason,
  ): EntryQueryCompositionResult => ({ status: "inexpressible", reason });
  const scope = input.query.scope;
  if (scope.kind !== "form" || !scope.form_id) {
    return unsupported("entryQueryToolSave.needsForm");
  }
  const form = input.form;
  if (!form?.id || !isUuid(form.id)) {
    return unsupported("entryQueryToolSave.needsForm");
  }
  if (form.id !== scope.form_id) {
    return unsupported("entryQueryToolSave.unsupportedQuery");
  }
  const knownForms = input.knownForms ?? [];

  let text: string | undefined;
  if (input.query.text !== undefined) {
    if (typeof input.query.text !== "string") {
      return unsupported("entryQueryToolSave.unsupportedQuery");
    }
    if (input.query.text !== "") text = input.query.text;
  }

  const filters: EntryQueryCompositionFilter[] = [];
  for (const filter of input.query.filters) {
    if (filter.field.kind !== "property") {
      return unsupported("entryQueryToolSave.unsupportedQuery");
    }
    if (!compositionOperators.has(filter.operator)) {
      return unsupported("entryQueryToolSave.unsupportedQuery");
    }
    if (!isScalarLiteral(filter.value)) {
      return unsupported("entryQueryToolSave.unsupportedQuery");
    }
    filters.push({
      field_id: filter.field.field_id,
      operator: filter.operator,
      value: filter.value,
    });
  }

  const sort: EntryQueryCompositionSort[] = [];
  for (const clause of input.query.sort) {
    if (clause.field.kind !== "property") {
      return unsupported("entryQueryToolSave.unsupportedQuery");
    }
    if (!compositionDirections.has(clause.direction)) {
      return unsupported("entryQueryToolSave.unsupportedQuery");
    }
    sort.push({
      field_id: clause.field.field_id,
      direction: clause.direction,
    });
  }

  // System refs in a projection are row identity (always returned outside
  // the projection payload), so they are dropped rather than snapshotted.
  // Dropping a filter or sort would change the result and stays fatal above.
  const projectedFieldIds: number[] = [];
  let projection: EntryQueryCompositionProjection = { kind: "preview" };
  if (input.projection.kind === "fields") {
    for (const field of input.projection.fields) {
      if (field.kind !== "property") continue;
      projectedFieldIds.push(field.field_id);
    }
    // An empty fields list is rejected by EntryQuery validation, so a
    // system-ref-only projection has no exact Composition grammar.
    if (projectedFieldIds.length === 0) {
      return unsupported("entryQueryToolSave.unsupportedQuery");
    }
    projection = { kind: "fields", fields: [...projectedFieldIds] };
  }

  const usedFieldIds = new Set<number>();
  for (const filter of filters) usedFieldIds.add(filter.field_id);
  for (const clause of sort) usedFieldIds.add(clause.field_id);
  for (const fieldId of projectedFieldIds) usedFieldIds.add(fieldId);

  // The resolver checks every query-used field against the snapshot and
  // reports `source_schema_changed` for gaps. Query-used means:
  // explicit filter/sort/projection fields, plus every text-searchable field
  // when `text` is set, plus every field when the projection is `preview`
  // (preview returns the whole row, so the snapshot must too).
  if (text !== undefined) {
    for (const field of Object.values(form.fields ?? {})) {
      const fieldId = formFieldId(field);
      if (fieldId === undefined) continue;
      if (textSearchesFieldType(formFieldType(field))) {
        usedFieldIds.add(fieldId);
      }
    }
  }
  if (projection.kind === "preview") {
    for (const field of Object.values(form.fields ?? {})) {
      const fieldId = formFieldId(field);
      if (fieldId !== undefined) usedFieldIds.add(fieldId);
    }
  }

  const fieldSchema: EntryQueryCompositionFieldSchemaEntry[] = [];
  for (const fieldId of [...usedFieldIds].sort((a, b) => a - b)) {
    const entry = schemaEntryForField(form, knownForms, fieldId);
    if (!entry) return unsupported("entryQueryToolSave.unsupportedQuery");
    fieldSchema.push(entry);
  }

  return {
    status: "ok",
    source: {
      id: "entry_rows",
      kind: "entry_query",
      form_id: scope.form_id,
      query: {
        ...(text !== undefined ? { text } : {}),
        filters,
        sort,
        projection,
      },
    },
    fieldSchema,
    warnings: [],
  };
};

export interface EntryQueryCompositionDocument {
  format: "ugoite.composition";
  format_version: 1;
  kind: "dashboard";
  name: string;
  tags: string[];
  spec: {
    parameters: [];
    sources: Array<
      EntryQueryCompositionSource & {
        field_schema: EntryQueryCompositionFieldSchemaEntry[];
      }
    >;
    components: Array<{ id: string; kind: "table"; source: string }>;
    layout: {
      kind: "flow";
      rows: Array<{
        id: string;
        items: Array<
          | { kind: "component"; component: string }
          | { kind: "parameter"; parameter: string }
        >;
      }>;
    };
  };
}

/** Build the smallest typed Composition that retains one EntryQuery view. */
export const buildEntryQueryCompositionDocument = (
  name: string,
  source: EntryQueryCompositionSource,
  fieldSchema: EntryQueryCompositionFieldSchemaEntry[],
): EntryQueryCompositionDocument => {
  const componentId = "results_table";
  return {
    format: "ugoite.composition",
    format_version: 1,
    kind: "dashboard",
    name: name.trim(),
    tags: [],
    spec: {
      parameters: [],
      sources: [{ ...source, field_schema: fieldSchema }],
      components: [{ id: componentId, kind: "table", source: source.id }],
      layout: {
        kind: "flow",
        rows: [{
          id: "main",
          items: [{ kind: "component", component: componentId }],
        }],
      },
    },
  };
};
