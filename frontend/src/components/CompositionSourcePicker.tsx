import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import {
  buildEntryQueryComposition,
  type EntryQueryCompositionFieldSchemaEntry,
} from "~/lib/entry-query-composition";
import {
  canCreateSavedSqlComposition,
  type CompositionParameterType,
  type CompositionResultType,
} from "~/lib/composition-api";
import type {
  DraftEntryQuerySeed,
  DraftSavedSqlSeed,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import { normalizeSqlVariables } from "~/lib/sql";
import { displaySqlName } from "~/lib/sql-metadata";
import { formApi, sqlApi } from "~/lib/ugoite-client";
import type { Form, SqlEntry } from "~/lib/types";

export type CompositionSourceSeed =
  | { kind: "saved_sql"; seed: DraftSavedSqlSeed }
  | { kind: "entry_query"; seed: DraftEntryQuerySeed };

interface CompositionSourcePickerProps {
  spaceId: string;
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
 * Data-source picker for the Composition Studio. Lists Forms and Saved SQL
 * (user-query only) with full-row selection and human names only. Seeds
 * reuse the existing builders: EntryQuery seeds go through
 * `buildEntryQueryComposition` with a preview projection (fail-closed),
 * Saved SQL seeds pin the exact revision with server-owned column types
 * from a bounded probe page (json fallback, unique columns required).
 */
export function CompositionSourcePicker(props: CompositionSourcePickerProps) {
  const titleId = "composition-source-picker-title";
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
        forms = listedForms.filter((form) => Boolean(form.id));
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
      const result = buildEntryQueryComposition({
        query: {
          scope: { kind: "form", form_id: form.id },
          filters: [],
          sort: [],
        },
        projection: { kind: "preview" },
        form,
        knownForms: forms,
      });
      if (result.status !== "ok") {
        setAddError(t(result.reason));
        return;
      }
      const fieldSchema: EntryQueryCompositionFieldSchemaEntry[] =
        result.fieldSchema;
      props.onSelect({
        kind: "entry_query",
        seed: {
          formId: form.id,
          name: form.name,
          fieldSchema,
          query: {
            filters: [],
            sort: [],
            projection: { kind: "preview" },
          },
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
          class="ui-dialog"
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
              <div class="ui-stack-sm">
                <h3 class="ui-label">{t("composition.studioForms")}</h3>
                <Show
                  when={sources().forms.length > 0}
                  fallback={
                    <p class="ui-muted">{t("composition.studioNoForms")}</p>
                  }
                >
                  <RowList label={t("composition.studioForms")}>
                    <For each={sources().forms}>
                      {(form) => (
                        <RowListItem
                          main={
                            <RowListButton
                              primary={
                                <span class="rowListName">
                                  <UiIcon name="forms" />
                                  <span>{form.name}</span>
                                </span>
                              }
                              onActivate={() => addEntryQuery(form)}
                            />
                          }
                        />
                      )}
                    </For>
                  </RowList>
                </Show>
                <h3 class="ui-label">{t("spaceShell.title.savedSql")}</h3>
                <Show
                  when={sources().queries.length > 0}
                  fallback={
                    <p class="ui-muted">{t("composition.studioNoSavedSql")}</p>
                  }
                >
                  <RowList label={t("spaceShell.title.savedSql")}>
                    <For each={sources().queries}>
                      {(entry) => (
                        <RowListItem
                          main={
                            <RowListButton
                              primary={
                                <span class="rowListName">
                                  <UiIcon name="sql" />
                                  <span>{displaySqlName(entry)}</span>
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
