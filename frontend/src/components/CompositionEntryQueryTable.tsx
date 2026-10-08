import { Show } from "solid-js";
import { EntryResultTable } from "./EntryResultTable";
import { LocalBusyIndicator } from "./LocalBusyIndicator";
import { ResultPagination } from "./ResultPagination";
import { formatDateLabel } from "~/lib/date-format";
import { formatValueForDisplay } from "~/lib/display-value";
import {
  type EntryFieldRef,
  type EntryProjection,
  type EntryQueryResult,
  systemEntryCapabilities,
} from "~/lib/entry-query";
import type {
  CompositionResolvedComponentBinding,
  CompositionResolvedSource,
} from "~/lib/composition-api";
import { t } from "~/lib/i18n";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";

export interface EntryDisplayColumn {
  key: string;
  label: string;
  text: (row: EntryQueryResult) => string;
}

const previewText = (row: EntryQueryResult): string =>
  row.preview?.trim() || "—";

const instantText = (micros: number): string =>
  formatDateLabel(new Date(micros / 1_000).toISOString());

const propertyText = (row: EntryQueryResult, name: string): string => {
  const value = row.properties?.[name];
  if (value === null || value === undefined) return "—";
  return formatValueForDisplay(value);
};

const timestampLabel = (kind: "created_at" | "updated_at"): string =>
  systemEntryCapabilities({ kind: "form", form_id: "" }).fields.find(
    (capability) => capability.field.kind === kind,
  )?.name ?? kind;

/**
 * Entry display columns for a Composition entry_query source, using the
 * same presentation vocabulary as EntryBrowser. Preview projections render
 * Preview/Created/Updated; fields projections render projected properties
 * in projection order with timestamps last. Property names resolve through
 * authorized Form metadata supplied by the route; unresolved properties use
 * their own projected row keys without guessing a field-to-key position.
 */
export function entryQueryDisplayColumns(
  source: Extract<CompositionResolvedSource, { kind: "entry_query" }>,
  rows: readonly EntryQueryResult[],
  fieldNames?: (
    formId: string,
    fieldId: number,
    sourceId?: string,
  ) => string | undefined,
  fieldKeys?: (
    formId: string,
    fieldId: number,
    sourceId?: string,
  ) => string | undefined,
): EntryDisplayColumn[] {
  const projection: EntryProjection = source.request.projection;
  if (projection.kind === "preview") {
    return [
      { key: "preview", label: t("entryBrowser.preview"), text: previewText },
      {
        key: "created_at",
        label: timestampLabel("created_at"),
        text: (row) => instantText(row.created_at_micros),
      },
      {
        key: "updated_at",
        label: timestampLabel("updated_at"),
        text: (row) => instantText(row.updated_at_micros),
      },
    ];
  }
  const formId = source.request.query.scope.kind === "form"
    ? source.request.query.scope.form_id
    : undefined;
  const regular: EntryDisplayColumn[] = [];
  const timestamps: EntryDisplayColumn[] = [];
  let unresolvedProjectedProperties = 0;
  const hasCreatedAt = projection.fields.some((field) =>
    field.kind === "created_at"
  );
  const hasUpdatedAt = projection.fields.some((field) =>
    field.kind === "updated_at"
  );
  const rowPropertyKeys = rows.length > 0
    ? Object.keys(rows[0].properties ?? {}).filter((key) =>
      key !== "form_id" &&
      !(key === "created_at_micros" && hasCreatedAt) &&
      !(key === "updated_at_micros" && hasUpdatedAt)
    )
    : [];
  const resolvedPropertyKeys = new Set<string>();
  for (const field of projection.fields) {
    const label = field.kind === "property" && formId && fieldNames
      ? fieldNames(formId, field.field_id, source.source_id)
      : undefined;
    const resolvedPropertyKey = field.kind === "property" && formId && fieldKeys
      ? fieldKeys(formId, field.field_id, source.source_id)
      : undefined;
    const propertyKey = resolvedPropertyKey ?? (
      label && rowPropertyKeys.includes(label) ? label : undefined
    );
    const column = displayColumnForField(
      field,
      label,
      propertyKey,
    );
    if (column === "unresolved") {
      unresolvedProjectedProperties += 1;
      continue;
    }
    if (!column) continue;
    if (field.kind === "created_at" || field.kind === "updated_at") {
      if (!timestamps.some((existing) => existing.key === column.key)) {
        timestamps.push(column);
      }
    } else {
      regular.push(column);
    }
    if (field.kind === "property" && propertyKey) {
      resolvedPropertyKeys.add(propertyKey);
    }
  }
  // Timestamps keep the canonical Created-then-Updated order, matching
  // EntryBrowser regardless of projection encounter order.
  timestamps.sort((left, right) =>
    left.key === right.key ? 0 : left.key === "created_at" ? -1 : 1
  );
  const fallback = unresolvedProjectedProperties > 0
    ? rowPropertyKeys.filter((key) => !resolvedPropertyKeys.has(key)).map(
      (name) => ({
        key: name,
        label: name,
        text: (row) => propertyText(row, name),
      }),
    )
    : [];
  return [...regular, ...fallback, ...timestamps];
}

const displayColumnForField = (
  field: EntryFieldRef,
  label: string | undefined,
  propertyKey: string | undefined,
): EntryDisplayColumn | null | "unresolved" => {
  if (field.kind === "created_at" || field.kind === "updated_at") {
    return {
      key: field.kind,
      label: timestampLabel(field.kind),
      text: (row) =>
        instantText(
          field.kind === "created_at"
            ? row.created_at_micros
            : row.updated_at_micros,
        ),
    };
  }
  if (field.kind === "property") {
    if (!propertyKey) return "unresolved";
    return {
      key: `property:${field.field_id}`,
      label: label === propertyKey ? label : label ?? propertyKey,
      text: (row) => propertyText(row, propertyKey),
    };
  }
  return null;
};

export function CompositionEntryQueryTable(props: {
  binding: Extract<CompositionResolvedComponentBinding, { kind: "table" }>;
  source: Extract<CompositionResolvedSource, { kind: "entry_query" }>;
  sourceState?: CompositionSourcePageState;
  ownsSourceStatus: boolean;
  fieldNames?: (
    formId: string,
    fieldId: number,
    sourceId?: string,
  ) => string | undefined;
  fieldKeys?: (
    formId: string,
    fieldId: number,
    sourceId?: string,
  ) => string | undefined;
  onNext: () => void;
  onPrevious: () => void;
  onRetry: () => void;
}) {
  const page = () => {
    const state = props.sourceState?.page;
    return state?.kind === "entry_query" ? state.page : undefined;
  };
  const rows = () => page()?.rows ?? [];
  const columns = (): EntryDisplayColumn[] =>
    entryQueryDisplayColumns(
      props.source,
      rows(),
      props.fieldNames,
      props.fieldKeys,
    );
  const sourceId = () => props.binding.source_id;
  const status = () => props.sourceState?.status;
  const loading = () =>
    props.ownsSourceStatus &&
    (!props.sourceState || status() === "loading");
  const failed = () => props.ownsSourceStatus && status() === "error";

  return (
    <section class="section">
      <Show when={props.binding.label}>
        <h2>{props.binding.label}</h2>
      </Show>
      <Show
        when={props.ownsSourceStatus || status() === "ready"}
      >
        <Show when={loading()}>
          <LocalBusyIndicator label={t("composition.queryLoading")} />
        </Show>
        <Show when={failed()}>
          <p class="ui-text-danger" role="alert">
            {t("composition.queryFailed")}
          </p>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            onClick={props.onRetry}
          >
            {t("composition.retry")}
          </button>
        </Show>
        <Show when={status() === "ready" && rows().length === 0 && !failed()}>
          <p class="ui-muted">{t("composition.queryEmpty")}</p>
        </Show>
        <Show when={!failed() && rows().length > 0}>
          <EntryResultTable
            columns={columns().map((column) => ({
              key: column.key,
              label: column.label,
              cell: (row) => (
                <span title={column.text(row)}>{column.text(row)}</span>
              ),
            }))}
            rows={rows()}
            pageIdentity={`${sourceId()}:${
              props.sourceState?.cursor ?? "first"
            }`}
            tableLabel={props.binding.label ?? t("composition.resultPages")}
          />
        </Show>
        <ResultPagination
          canPrevious={(props.sourceState?.cursorStack.length ?? 0) > 1}
          canNext={!!page()?.has_more && !!page()?.next}
          busy={status() !== "ready"}
          previousLabel={t("composition.previous")}
          nextLabel={t("composition.next")}
          ariaLabel={t("composition.resultPages")}
          onPrevious={props.onPrevious}
          onNext={props.onNext}
        />
      </Show>
    </section>
  );
}
