import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { FormRowLabel, SavedSqlRowLabel } from "~/components/SourceRowLabels";
import {
  buildEntryQueryComposition,
  buildEntryQueryCompositionFieldSchema,
  type EntryQueryCompositionFieldSchemaEntry,
  MAX_COMPOSITION_FIELD_SCHEMA_ITEMS,
} from "~/lib/entry-query-composition";
import {
  canCreateSavedSqlComposition,
  type CompositionParameterType,
  type CompositionResultType,
} from "~/lib/composition-api";
import {
  type DraftEntryQuerySeed,
  type DraftSavedSqlSeed,
  type DraftSource,
  MAX_ENTRY_PROJECTION_FIELDS,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { normalizeSqlVariables } from "~/lib/sql";
import { studioCapabilitiesFromForm } from "~/lib/entry-query-studio-capabilities";
import { displaySqlName } from "~/lib/sql-metadata";
import { formApi, sqlApi } from "~/lib/ugoite-client";
import type { Form, SqlEntry } from "~/lib/types";

export type CompositionSourceSeed =
  | { kind: "saved_sql"; seed: DraftSavedSqlSeed }
  | { kind: "entry_query"; seed: DraftEntryQuerySeed };

interface CompositionSourcePickerProps {
  spaceId: string;
  /** Sources already referenced by the current draft; shown first for reuse. */
  existingSources?: readonly DraftSource[];
  onSelectExisting?: (sourceDraftId: string) => void;
  onSelect: (seed: CompositionSourceSeed) => void;
  onClose: () => void;
}

type PickerLoad =
  | { status: "loading" }
  | { status: "ready"; forms: Form[]; queries: SqlEntry[] }
  | { status: "error" };

const isAbort = (error: unknown): boolean =>
  !!error && typeof error === "object" &&
  (error as { name?: unknown }).name === "AbortError";

/**
 * Data-source picker for the Composition Studio. Forms and Saved SQL share
 * one dialog behind Forms | Saved SQL tabs (the settings tablist pattern)
 * reusing the list pages' shared row labels with full-row selection and
 * human names only. Internal registry forms stay hidden through the shared
 * `filterCreatableEntryForms` single source. Seeds reuse the existing
 * builders: EntryQuery seeds go through `buildEntryQueryComposition` with
 * projectable Form fields (Preview only when none are projectable), Saved SQL
 * seeds pin the exact revision
 * with server-owned column types from a bounded probe page (json fallback,
 * unique columns required).
 */
export function CompositionSourcePicker(props: CompositionSourcePickerProps) {
  const titleId = "composition-source-picker-title";
  const formsTabId = "composition-source-tab-forms";
  const formsPanelId = "composition-source-panel-forms";
  const sqlTabId = "composition-source-tab-saved-sql";
  const sqlPanelId = "composition-source-panel-saved-sql";
  const [tab, setTab] = createSignal<"forms" | "savedSql">("forms");
  const [load, setLoad] = createSignal<PickerLoad>({ status: "loading" });
  const [adding, setAdding] = createSignal(false);
  const [addError, setAddError] = createSignal<string | null>(null);
  let dialog: HTMLDivElement | undefined;
  let opener: HTMLElement | null = null;
  let generation = 0;
  let controller: AbortController | undefined;
  let forms: Form[] = [];

  const spaceId = () => props.spaceId;

  const reload = () => {
    const requestGeneration = ++generation;
    controller?.abort();
    const current = new AbortController();
    controller = current;
    setLoad({ status: "loading" });
    void Promise.all([
      formApi.list(spaceId()),
      sqlApi.list(spaceId()),
    ]).then(
      ([listedForms, listedSql]) => {
        if (
          requestGeneration !== generation || current.signal.aborted
        ) return;
        forms = filterCreatableEntryForms(
          listedForms.filter((form) => Boolean(form.id)),
        );
        setLoad({
          status: "ready",
          forms,
          queries: listedSql.filter((entry) =>
            canCreateSavedSqlComposition(entry)
          ),
        });
      },
      () => {
        if (
          requestGeneration !== generation || current.signal.aborted
        ) return;
        setLoad({ status: "error" });
      },
    );
  };

  reload();
  onCleanup(() => {
    generation += 1;
    controller?.abort();
  });

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    queueMicrotask(() => dialog?.querySelector("button")?.focus());
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
      const target = opener;
      opener = null;
      queueMicrotask(() => {
        if (target?.isConnected) target.focus();
      });
    });
  });

  const locked = () => adding();
  const close = () => {
    if (!locked()) props.onClose();
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key === "Tab") {
      const container = event.currentTarget as HTMLElement;
      const focusable = Array.from(
        container.querySelectorAll<HTMLElement>(
          "button:not([disabled])",
        ),
      );
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }
      const currentIndex = focusable.indexOf(
        document.activeElement as HTMLElement,
      );
      const nextIndex = event.shiftKey
        ? currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1
        : currentIndex < 0 || currentIndex === focusable.length - 1
        ? 0
        : currentIndex + 1;
      event.preventDefault();
      focusable[nextIndex].focus();
    }
  };

  const addEntryQuery = (form: Form) => {
    if (adding() || !form.id) return;
    setAdding(true);
    setAddError(null);
    try {
      const projectionFields = [];
      const selectedFields = new Set<number>();
      for (const capability of studioCapabilitiesFromForm(form)) {
        if (
          !capability.projectable || capability.field.kind !== "property" ||
          selectedFields.has(capability.field.field_id)
        ) continue;
        selectedFields.add(capability.field.field_id);
        projectionFields.push(capability.field);
        if (projectionFields.length >= MAX_ENTRY_PROJECTION_FIELDS) break;
      }
      const projection = projectionFields.length > 0
        ? { kind: "fields" as const, fields: projectionFields }
        : { kind: "preview" as const };
      const result = buildEntryQueryComposition({
        query: {
          scope: { kind: "form", form_id: form.id },
          filters: [],
          sort: [],
        },
        projection,
        form,
        knownForms: forms,
      });
      if (result.status !== "ok") {
        setAddError(t(result.reason));
        return;
      }
      const schemaEntries = buildEntryQueryCompositionFieldSchema(
        form,
        forms,
      );
      if (
        projection.kind === "preview" &&
        schemaEntries.length > MAX_COMPOSITION_FIELD_SCHEMA_ITEMS
      ) {
        setAddError("entryQueryToolSave.unsupportedQuery");
        return;
      }
      const schemaById = new Map(
        schemaEntries.map((entry) => [entry.field_id, entry]),
      );
      const selectedSchema = projectionFields.flatMap((field) => {
        const entry = schemaById.get(field.field_id);
        return entry ? [entry] : [];
      });
      const initialFieldIds = new Set(
        selectedSchema.map((entry) => entry.field_id),
      );
      const fieldSchema: EntryQueryCompositionFieldSchemaEntry[] = [
        ...selectedSchema,
        ...schemaEntries.filter((entry) =>
          !initialFieldIds.has(entry.field_id)
        ),
      ].slice(0, MAX_COMPOSITION_FIELD_SCHEMA_ITEMS).sort((left, right) =>
        left.field_id - right.field_id
      );
      props.onSelect({
        kind: "entry_query",
        seed: {
          formId: form.id,
          name: form.name,
          fieldSchema,
          query: result.source.query,
        },
      });
    } finally {
      setAdding(false);
    }
  };

  const addSavedSql = async (entry: SqlEntry) => {
    if (adding()) return;
    setAdding(true);
    setAddError(null);
    try {
      const current = await sqlApi.get(spaceId(), entry.id);
      if (!canCreateSavedSqlComposition(current)) {
        setAddError(t("composition.studioCannotAdd"));
        return;
      }
      // Bounded probe for the server-owned column descriptor. The Browser
      // never infers types from row values; columns absent from the schema
      // keep the json fallback, mirroring buildSavedSqlCompositionDocument.
      const page = await sqlApi.query(spaceId(), {
        sql: normalizeSqlVariables(current.sql).sql,
        parameters: {},
        parameter_types: {},
        limit: 1,
        saved_sql: { id: current.id, revision_id: current.revision_id },
      });
      const columns = [...page.columns];
      if (
        columns.length === 0 || new Set(columns).size !== columns.length
      ) {
        setAddError(t("composition.studioCannotAdd"));
        return;
      }
      const schemaByName = new Map(
        (page.result_schema ?? []).map((column) => [column.name, column.type]),
      );
      const expectedResult = columns.map((name) => ({
        name,
        type: (schemaByName.get(name) ?? "json") as CompositionResultType,
      }));
      props.onSelect({
        kind: "saved_sql",
        seed: {
          entryId: current.id,
          revisionId: current.revision_id,
          name: displaySqlName(current),
          expectedResult,
          variables: Object.fromEntries(
            current.variables.map((variable) => [
              variable.name,
              { parameter: variable.name },
            ]),
          ),
          variableTypes: Object.fromEntries(
            current.variables.map((variable) => [
              variable.name,
              variable.type as CompositionParameterType,
            ]),
          ),
        },
      });
    } catch (error) {
      if (!isAbort(error)) setAddError(t("composition.studioCannotAdd"));
    } finally {
      setAdding(false);
    }
  };

  const ready = () => {
    const current = load();
    return current.status === "ready" ? current : undefined;
  };

  return (
    <Portal>
      <div
        class="ui-backdrop"
        onClick={(event) => {
          if (event.target === event.currentTarget) close();
        }}
      >
        <div
          ref={dialog}
          class="ui-dialog composition-source-picker"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          onKeyDown={handleKeyDown}
        >
          <h2 id={titleId} class="ui-dialog-title">
            {t("composition.studioAddData")}
          </h2>
          <Show when={adding()}>
            <LocalBusyIndicator label={t("composition.studioSourcesLoading")} />
          </Show>
          <Show when={load().status === "loading" && !adding()}>
            <LocalBusyIndicator label={t("composition.studioSourcesLoading")} />
          </Show>
          <Show when={load().status === "error"}>
            <p class="ui-text-danger" role="alert">
              {t("composition.studioSourcesFailed")}
            </p>
            <button
              class="ui-button ui-button-secondary"
              type="button"
              onClick={reload}
            >
              {t("composition.retry")}
            </button>
          </Show>
          <Show when={ready()}>
            {(sources) => (
              <div>
                <Show when={(props.existingSources?.length ?? 0) > 0}>
                  <section class="ui-stack-sm">
                    <h3 class="ui-label">
                      {t("composition.studioCurrentSources")}
                    </h3>
                    <RowList label={t("composition.studioCurrentSources")}>
                      <For each={props.existingSources ?? []}>
                        {(source) => (
                          <RowListItem
                            main={
                              <RowListButton
                                ariaLabel={`${source.name}, ${
                                  source.kind === "saved_sql"
                                    ? t("spaceShell.title.savedSql")
                                    : t("composition.studioForms")
                                }`}
                                title={source.name}
                                primary={
                                  <span class="rowListName">{source.name}</span>
                                }
                                secondary={source.kind === "saved_sql"
                                  ? t("spaceShell.title.savedSql")
                                  : t("composition.studioForms")}
                                onActivate={() =>
                                  props.onSelectExisting?.(source.draftId)}
                              />
                            }
                          />
                        )}
                      </For>
                    </RowList>
                  </section>
                </Show>
                <div
                  class="tabs"
                  role="tablist"
                  aria-labelledby={titleId}
                >
                  <button
                    type="button"
                    role="tab"
                    id={formsTabId}
                    aria-selected={tab() === "forms"}
                    aria-controls={formsPanelId}
                    class="tab"
                    classList={{ active: tab() === "forms" }}
                    onClick={() => setTab("forms")}
                  >
                    {t("composition.studioForms")}
                  </button>
                  <button
                    type="button"
                    role="tab"
                    id={sqlTabId}
                    aria-selected={tab() === "savedSql"}
                    aria-controls={sqlPanelId}
                    class="tab"
                    classList={{ active: tab() === "savedSql" }}
                    onClick={() => setTab("savedSql")}
                  >
                    {t("spaceShell.title.savedSql")}
                  </button>
                </div>
                <Show when={tab() === "forms"}>
                  <section
                    id={formsPanelId}
                    role="tabpanel"
                    aria-labelledby={formsTabId}
                    class="ui-stack-sm"
                  >
                    <Show
                      when={sources().forms.length > 0}
                      fallback={
                        <p class="ui-muted">
                          {t("composition.studioNoForms")}
                        </p>
                      }
                    >
                      <RowList
                        label={t("composition.studioForms")}
                        labelledBy={formsTabId}
                      >
                        <For each={sources().forms}>
                          {(form) => (
                            <RowListItem
                              main={
                                <RowListButton
                                  title={form.name}
                                  primary={<FormRowLabel name={form.name} />}
                                  onActivate={() => addEntryQuery(form)}
                                />
                              }
                            />
                          )}
                        </For>
                      </RowList>
                    </Show>
                  </section>
                </Show>
                <Show when={tab() === "savedSql"}>
                  <section
                    id={sqlPanelId}
                    role="tabpanel"
                    aria-labelledby={sqlTabId}
                    class="ui-stack-sm"
                  >
                    <Show
                      when={sources().queries.length > 0}
                      fallback={
                        <p class="ui-muted">
                          {t("composition.studioNoSavedSql")}
                        </p>
                      }
                    >
                      <RowList
                        label={t("spaceShell.title.savedSql")}
                        labelledBy={sqlTabId}
                      >
                        <For each={sources().queries}>
                          {(entry) => (
                            <RowListItem
                              main={
                                <RowListButton
                                  title={displaySqlName(entry)}
                                  primary={
                                    <span class="rowListName">
                                      <SavedSqlRowLabel entry={entry} />
                                    </span>
                                  }
                                  onActivate={() => void addSavedSql(entry)}
                                />
                              }
                            />
                          )}
                        </For>
                      </RowList>
                    </Show>
                  </section>
                </Show>
              </div>
            )}
          </Show>
          <Show when={addError()}>
            <p class="ui-text-danger" role="alert">{addError()}</p>
          </Show>
          <div class="ui-dialog-actions">
            <button
              type="button"
              class="ui-button ui-button-secondary"
              disabled={adding()}
              onClick={close}
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
}
