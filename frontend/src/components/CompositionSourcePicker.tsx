import {
  createEffect,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { Portal } from "solid-js/web";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { FormRowLabel, SavedSqlRowLabel } from "~/components/SourceRowLabels";
import {
  canCreateSavedSqlComposition,
  compositionResultTypeFromSql,
  type CompositionVariableType,
} from "~/lib/composition-api";
import {
  compositionResultSchemaFingerprint,
  type DraftInlineSqlSeed,
  type DraftSavedSqlSeed,
  type DraftSource,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { normalizeSqlVariables } from "~/lib/sql";
import { displaySqlName } from "~/lib/sql-metadata";
import { formApi, sqlApi } from "~/lib/ugoite-client";
import type { Form, SqlEntry } from "~/lib/types";

export type CompositionSourceSeed =
  | { kind: "saved_sql"; seed: DraftSavedSqlSeed }
  | { kind: "inline_sql"; seed: DraftInlineSqlSeed };

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

const sqlProbeDefault = (type: string): string | number | boolean => {
  switch (type) {
    case "boolean":
      return false;
    case "integer":
    case "float":
      return 0;
    case "date":
      return "1970-01-01";
    case "timestamp":
      return "1970-01-01T00:00:00Z";
    default:
      return "";
  }
};

/**
 * Data-source picker for the Composition Studio. Forms and Saved SQL share
 * one dialog behind Forms | Saved SQL tabs (the settings tablist pattern)
 * reusing the list pages' shared row labels with full-row selection and
 * human names only. Internal registry forms stay hidden through the shared
 * `filterCreatableEntryForms` single source. A Form row creates editable
 * inline SQL that names the Form relation; a Saved SQL row pins one exact
 * revision with the server-owned result descriptor.
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
  const reload = (currentSpaceId = spaceId()) => {
    const requestGeneration = ++generation;
    controller?.abort();
    const current = new AbortController();
    controller = current;
    setLoad({ status: "loading" });
    void Promise.all([
      formApi.list(currentSpaceId),
      sqlApi.list(currentSpaceId),
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

  createEffect(() => {
    reload(spaceId());
  });
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

  const addInlineSql = (form: Form) => {
    if (adding() || !form.id) return;
    setAdding(true);
    setAddError(null);
    try {
      const relation = `"${form.name.replaceAll('"', '""')}"`;
      props.onSelect({
        kind: "inline_sql",
        seed: {
          name: form.name,
          sql: `SELECT * FROM ${relation}`,
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
      const normalized = normalizeSqlVariables(current.sql, current.variables);
      const page = await sqlApi.query(spaceId(), {
        sql: normalized.sql,
        parameters: Object.fromEntries(normalized.variables.map((variable) => [
          variable.name,
          variable.default ?? sqlProbeDefault(variable.type),
        ])),
        parameter_types: Object.fromEntries(normalized.variables.map((variable) => [
          variable.name,
          variable.type,
        ])),
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
        type: compositionResultTypeFromSql(schemaByName.get(name)),
      }));
      const schemaFingerprint = await compositionResultSchemaFingerprint(
        expectedResult,
      );
      props.onSelect({
        kind: "saved_sql",
        seed: {
          entryId: current.id,
          revisionId: current.revision_id,
          name: displaySqlName(current),
          sql: current.sql,
          expectedResult,
          schemaFingerprint,
          variables: {},
          variableTypes: Object.fromEntries(
            current.variables.map((variable) => [
              variable.name,
              variable.type as CompositionVariableType,
            ]),
          ),
          variableDefaults: Object.fromEntries(
            current.variables.flatMap((variable) =>
              variable.default === undefined
                ? []
                : [[variable.name, variable.default]]
            ),
          ),
        },
      });
    } catch (error) {
      if (!isAbort(error)) setAddError(t("composition.studioCannotAdd"));
    } finally {
      setAdding(false);
    }
  };

  const addBlankSql = () => {
    props.onSelect({
      kind: "inline_sql",
      seed: { name: t("sqlPage.sql"), sql: "SELECT 1" },
    });
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
            {t("composition.studioChooseSource")}
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
                      <For
                        each={props.existingSources ?? []}
                      >
                        {(source) => (
                          <RowListItem
                            main={
                              <RowListButton
                                ariaLabel={`${source.name}, ${
                                  source.kind === "saved_sql"
                                    ? t("spaceShell.title.savedSql")
                                    : t("sqlPage.sql")
                                }`}
                                title={source.name}
                                primary={
                                  <span class="rowListName">{source.name}</span>
                                }
                                secondary={source.kind === "saved_sql"
                                  ? t("spaceShell.title.savedSql")
                                  : t("sqlPage.sql")}
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
                <button
                  class="btn secondary"
                  type="button"
                  onClick={addBlankSql}
                >
                  {t("sqlPage.sql")}
                </button>
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
                                  onActivate={() => addInlineSql(form)}
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
