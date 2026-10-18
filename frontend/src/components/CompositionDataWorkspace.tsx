import { createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
import { UiIcon } from "~/components/UiIcon";
import { A } from "@solidjs/router";
import { SourceNavigator } from "~/components/SourceNavigator";
import { SQLWorkspace } from "~/components/SQLWorkspace";
import type { SQLWorkspaceAction } from "~/components/SQLWorkspace";
import { SqlStructureToolbar } from "~/components/SqlStructureToolbar";
import type { CompositionResolvedSource } from "~/lib/composition-api";
import type { CompositionDraft, DraftVariable } from "~/lib/composition-draft";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import type { CompositionSourceCountState } from "~/lib/composition-preview-handle";
import {
  buildSqlStructureContexts,
  normalizeSqlVariables,
  type SqlSchema,
} from "~/lib/sql";
import { t } from "~/lib/i18n";
import type { Form } from "~/lib/types";

export interface CompositionDataWorkspaceProps {
  draft: CompositionDraft;
  headingId: string;
  /** Selected source draft ID; the editor area renders its detail. */
  selectedSourceId: string | null;
  /** Split mode's narrow side pane can collapse its source list on desktop. */
  collapsible?: boolean;
  /** The mobile source drawer is coordinated by the Studio overlay owner. */
  mobileDrawerOpen?: boolean;
  onMobileDrawerOpenChange?: (open: boolean) => void;
  onSelectSource: (sourceDraftId: string) => void;
  onAddSource: (returnFocus?: () => HTMLElement | null) => void;
  onMoveSource: (sourceDraftId: string, direction: "up" | "down") => void;
  onRemoveSource: (sourceDraftId: string) => void;
  onInlineSql: (sourceDraftId: string, sql: string) => boolean;
  onInlineSqlName: (sourceDraftId: string, name: string) => boolean;
  variables: readonly DraftVariable[];
  variableValues: Readonly<Record<string, unknown>>;
  savedSqlEditHref: (entryId: string) => string;
  sqlSchema: SqlSchema;
  forms: readonly Form[];
  planSources: readonly CompositionResolvedSource[];
  sourceStates: Record<string, CompositionSourcePageState>;
  sourceCounts?: Record<string, CompositionSourceCountState>;
  /** True while the shared debounced preview is in flight. Result spinners
   * render only while active; a settled preview without a page for a source
   * renders nothing (the diagnostics strip already covers failures). */
  previewActive: boolean;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
  onCount?: (sourceId: string) => void;
  registerSourceRow: (sourceDraftId: string, el: HTMLDivElement | null) => void;
}

/**
 * Data workspace for the Composition Studio: a source navigator beside the
 * selected SQL Workspace. Inline SQL edits stay in the draft; Saved SQL stays
 * pinned to one exact revision. Selection and paging
 * stay transient Work; only the selected source renders an editor.
 */
export function CompositionDataWorkspace(props: CompositionDataWorkspaceProps) {
  const [structureActions, setStructureActions] = createSignal<
    readonly SQLWorkspaceAction[]
  >([]);
  const [desktopNavigatorOpen, setDesktopNavigatorOpen] = createSignal(true);
  const [localMobileDrawerOpen, setLocalMobileDrawerOpen] = createSignal(false);
  const [narrowViewport, setNarrowViewport] = createSignal(false);
  const [editingInlineSqlNameSourceId, setEditingInlineSqlNameSourceId] =
    createSignal<string | null>(null);
  const navigatorId = `${props.headingId}-source-navigator`;
  let hideNavigatorButton: HTMLButtonElement | undefined;
  let showNavigatorButton: HTMLButtonElement | undefined;
  let inlineSqlNameInput: HTMLInputElement | undefined;
  let inlineSqlNameEditButton: HTMLButtonElement | undefined;
  const mobileDrawerOpen = () =>
    props.mobileDrawerOpen ?? localMobileDrawerOpen();
  const setMobileDrawerOpen = (open: boolean) => {
    props.onMobileDrawerOpenChange?.(open);
    if (props.mobileDrawerOpen === undefined) setLocalMobileDrawerOpen(open);
  };
  const navigatorOpen = () =>
    narrowViewport() ? mobileDrawerOpen() : desktopNavigatorOpen();
  const setNavigatorOpen = (open: boolean) => {
    if (narrowViewport()) setMobileDrawerOpen(open);
    else setDesktopNavigatorOpen(open);
  };
  const selectedSource = () =>
    props.draft.sources.find((source) =>
      source.draftId === props.selectedSourceId
    );
  const selectedSourceIndex = () =>
    props.draft.sources.findIndex((source) =>
      source.draftId === props.selectedSourceId
    );
  const selectedSourceUsedByDisplay = () =>
    props.draft.displays.some((display) =>
      display.kind !== "text" &&
      display.sourceDraftId === props.selectedSourceId
    );

  const closeNavigator = () => {
    setNavigatorOpen(false);
    queueMicrotask(() => showNavigatorButton?.focus());
  };

  const handleNavigatorKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape" && narrowViewport()) {
      event.preventDefault();
      closeNavigator();
      return;
    }
    if (event.key !== "Tab" || !narrowViewport()) return;
    const container = event.currentTarget as HTMLElement;
    const focusable = Array.from(
      container.querySelectorAll<HTMLElement>("button:not([disabled])"),
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
  };

  let previousSelectedSourceId: string | null | undefined;

  createEffect(() => {
    const sourceId = props.selectedSourceId;
    const narrow = narrowViewport();
    const previous = previousSelectedSourceId;
    previousSelectedSourceId = sourceId;
    if (
      previous !== undefined && sourceId !== previous &&
      editingInlineSqlNameSourceId() !== null
    ) {
      setEditingInlineSqlNameSourceId(null);
    }
    if (narrow && sourceId && previous !== undefined && sourceId !== previous) {
      closeNavigator();
    }
  });

  onMount(() => {
    if (
      typeof window === "undefined" || typeof window.matchMedia !== "function"
    ) {
      return;
    }
    const media = window.matchMedia("(max-width: 560px)");
    const syncViewport = () => {
      setNarrowViewport(media.matches);
      setDesktopNavigatorOpen(true);
      setMobileDrawerOpen(media.matches && props.selectedSourceId === null);
    };
    syncViewport();
    if (media.matches && props.selectedSourceId !== null) {
      // A data jump can mount the workspace with its target already selected.
      // The mobile source drawer starts closed, so focus its visible reopen
      // control instead of a source row inside the hidden drawer.
      queueMicrotask(() => showNavigatorButton?.focus());
    } else if (media.matches) {
      queueMicrotask(() => hideNavigatorButton?.focus());
    }
    if (typeof media.addEventListener === "function") {
      media.addEventListener("change", syncViewport);
      onCleanup(() => media.removeEventListener("change", syncViewport));
    } else {
      media.addListener(syncViewport);
      onCleanup(() => media.removeListener(syncViewport));
    }
  });

  return (
    <div
      class="dataWorkspace"
      classList={{
        "dataWorkspace--navigator-collapsed": !navigatorOpen(),
        "dataWorkspace--mobile": narrowViewport(),
        "dataWorkspace--collapsible": props.collapsible,
        "dataWorkspace--mobile-navigator-open": narrowViewport() &&
          navigatorOpen(),
      }}
    >
      <Show when={narrowViewport() && navigatorOpen()}>
        <button
          class="dataWorkspaceDrawerBackdrop"
          type="button"
          aria-label={t("composition.studioHideDataSources")}
          onClick={closeNavigator}
        />
      </Show>
      <aside
        id={navigatorId}
        class="dataWorkspaceNavigator"
        aria-labelledby={props.headingId}
        role={narrowViewport() ? "dialog" : undefined}
        aria-modal={narrowViewport() ? "true" : undefined}
        onKeyDown={handleNavigatorKeyDown}
        hidden={!navigatorOpen()}
      >
        <div class="dataWorkspaceNavigatorActions">
          <button
            class="dataWorkspaceDrawerClose"
            type="button"
            ref={hideNavigatorButton}
            aria-label={t("composition.studioHideDataSources")}
            aria-expanded={navigatorOpen()}
            aria-controls={navigatorId}
            title={t("composition.studioHideDataSources")}
            onClick={() => {
              closeNavigator();
            }}
          >
            <UiIcon name="close" />
            <span class="ui-sr-only">
              {t("composition.studioHideDataSources")}
            </span>
          </button>
        </div>
        <SourceNavigator
          sources={props.draft.sources}
          headingId={props.headingId}
          selectedId={props.selectedSourceId}
          onSelect={(sourceDraftId) => {
            props.onSelectSource(sourceDraftId);
            if (narrowViewport()) closeNavigator();
          }}
          registerRow={props.registerSourceRow}
        />
        <div class="dataWorkspaceNavigatorFooter">
          <IconButton
            icon="plus"
            label={t("composition.studioAddSource")}
            onClick={() => {
              const returnFocus = narrowViewport()
                ? () => {
                  const trigger = document.getElementById(
                    `${props.headingId}-source-menu-button`,
                  );
                  return trigger instanceof HTMLElement ? trigger : null;
                }
                : () =>
                  document.activeElement instanceof HTMLElement
                    ? document.activeElement
                    : null;
              if (narrowViewport()) setMobileDrawerOpen(false);
              props.onAddSource(returnFocus);
            }}
          />
          <button
            type="button"
            class="pill iconpill icononly"
            disabled={selectedSourceIndex() <= 0}
            aria-label={t("composition.studioMoveUp", {
              name: selectedSource()?.name ?? t("composition.studioSource"),
            })}
            title={t("composition.studioMoveUp", {
              name: selectedSource()?.name ?? t("composition.studioSource"),
            })}
            onClick={() => {
              const source = selectedSource();
              if (source) props.onMoveSource(source.draftId, "up");
            }}
          >
            <span aria-hidden="true">↑</span>
          </button>
          <button
            type="button"
            class="pill iconpill icononly"
            disabled={selectedSourceIndex() < 0 ||
              selectedSourceIndex() === props.draft.sources.length - 1}
            aria-label={t("composition.studioMoveDown", {
              name: selectedSource()?.name ?? t("composition.studioSource"),
            })}
            title={t("composition.studioMoveDown", {
              name: selectedSource()?.name ?? t("composition.studioSource"),
            })}
            onClick={() => {
              const source = selectedSource();
              if (source) props.onMoveSource(source.draftId, "down");
            }}
          >
            <span aria-hidden="true">↓</span>
          </button>
          <IconButton
            icon="trash"
            label={selectedSource()
              ? t("composition.studioRemoveSource", {
                name: selectedSource()!.name,
              })
              : t("composition.studioRemoveSelectedSource")}
            title={selectedSourceUsedByDisplay()
              ? t("composition.studioSourceUsedByDisplay")
              : undefined}
            disabled={!selectedSource() || selectedSourceUsedByDisplay()}
            onClick={() => {
              const source = selectedSource();
              if (source && !selectedSourceUsedByDisplay()) {
                props.onRemoveSource(source.draftId);
              }
            }}
          />
        </div>
      </aside>
      <div
        class="dataWorkspaceMain"
        inert={narrowViewport() && navigatorOpen()}
      >
        <Show when={!navigatorOpen()}>
          <div class="dataWorkspaceMainActions">
            <button
              class="dataWorkspaceSourceMenuButton"
              type="button"
              id={`${props.headingId}-source-menu-button`}
              ref={showNavigatorButton}
              aria-label={t("composition.studioShowDataSources")}
              aria-expanded={navigatorOpen()}
              aria-controls={navigatorId}
              title={t("composition.studioShowDataSources")}
              onClick={() => {
                setNavigatorOpen(true);
                queueMicrotask(() => hideNavigatorButton?.focus());
              }}
            >
              <UiIcon name="menu" />
              <span>
                {selectedSource()?.name ?? t("composition.studioData")}
              </span>
              <span class="ui-sr-only">
                {t("composition.studioShowDataSources")}
              </span>
            </button>
          </div>
        </Show>
        <Show when={selectedSource()}>
          {(source) => {
            const selected = source;
            const sourceId = () => selected().draftId;
            const pageState = () => props.sourceStates[sourceId()];
            const page = () => {
              const value = pageState()?.page;
              return value?.kind === "sql" ? value.page : undefined;
            };
            const planSource = () =>
              props.planSources.find((item) => item.source_id === sourceId());
            const sqlVariables = () => {
              if (selected().kind === "inline_sql") {
                return normalizeSqlVariables(selected().sql).variables.map((
                  item,
                ) => ({
                  name: item.name,
                  type:
                    props.variables.find((variable) =>
                      variable.id === item.name
                    )?.type ?? item.type,
                  variableId: item.name,
                }));
              }
              const parameterTypes = planSource()?.request.parameter_types ??
                selected().variableTypes ?? {};
              return Object.entries(parameterTypes).flatMap(([name, type]) =>
                typeof type === "string"
                  ? [{
                    name,
                    type,
                    variableId: selected().variables[name]?.variable,
                  }]
                  : []
              );
            };
            const variables = () =>
              sqlVariables().map((item) => {
                const definition = props.variables.find((variable) =>
                  variable.id === item.variableId
                );
                return {
                  name: item.name,
                  type: item.type,
                  required: definition?.required,
                  default: definition?.default as
                    | string
                    | number
                    | boolean
                    | undefined,
                };
              });
            const structureContexts = () => {
              if (selected().kind !== "inline_sql") return [];
              const bindings = selected().formBindings ?? [];
              if (bindings.length === 0) return [];
              const boundForms = bindings.map((binding) =>
                props.forms.find((item) => item.id === binding.form_id)
              );
              // Do not give the editor a partial schema: an unresolved
              // relation must keep the whole query out of structure editing.
              if (boundForms.some((form) => !form)) return [];
              return buildSqlStructureContexts(
                boundForms.map((form, index) => ({
                  ...form!,
                  name: bindings[index].relation,
                })),
                variables(),
                `composition:${sourceId()}:${JSON.stringify(bindings)}`,
              );
            };
            const parameterValues = () =>
              Object.fromEntries(
                sqlVariables().flatMap((item) => {
                  const value =
                    props.variableValues[item.variableId ?? item.name];
                  return value === undefined
                    ? []
                    : [[item.name, value as string | number | boolean]];
                }),
              );
            const state = () => pageState()?.status;
            const status = () =>
              props.previewActive
                ? "loading" as const
                : state() === "ready"
                ? "success" as const
                : state() === "error"
                ? "error" as const
                : "idle" as const;
            return (
              <div
                class="dataWorkspaceEditor"
                role="region"
                aria-label={selected().name}
              >
                {selected().kind === "inline_sql"
                  ? (
                    <div class="dataWorkspaceSqlNameControl">
                      <Show
                        when={editingInlineSqlNameSourceId() === sourceId()}
                        fallback={
                          <IconButton
                            ref={(element) => inlineSqlNameEditButton = element}
                            icon="edit"
                            label={t("composition.studioRenameSqlSource", {
                              name: selected().name,
                            })}
                            onClick={() => {
                              setEditingInlineSqlNameSourceId(sourceId());
                              queueMicrotask(() => inlineSqlNameInput?.focus());
                            }}
                          />
                        }
                      >
                        <input
                          ref={(element) => inlineSqlNameInput = element}
                          class="ui-input dataWorkspaceSqlName"
                          aria-label={t("sqlPage.queryName")}
                          value={selected().name}
                          onInput={(event) =>
                            props.onInlineSqlName(
                              sourceId(),
                              event.currentTarget.value,
                            )}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") {
                              event.preventDefault();
                              setEditingInlineSqlNameSourceId(null);
                              queueMicrotask(() =>
                                inlineSqlNameEditButton?.focus()
                              );
                            }
                          }}
                          onBlur={() => {
                            if (editingInlineSqlNameSourceId() === sourceId()) {
                              setEditingInlineSqlNameSourceId(null);
                            }
                          }}
                        />
                      </Show>
                    </div>
                  )
                  : null}
                {selected().kind === "saved_sql"
                  ? (
                    <A
                      class="pill iconpill icononly dataWorkspaceSavedSqlEdit"
                      href={props.savedSqlEditHref(selected().entryId)}
                      aria-label={t("composition.studioEditSavedSql")}
                      title={t("composition.studioEditSavedSql")}
                    >
                      <UiIcon name="edit" />
                      <span class="ui-sr-only">
                        {t("composition.studioEditSavedSql")}
                      </span>
                    </A>
                  )
                  : null}
                <SQLWorkspace
                  id={`composition-${sourceId()}`}
                  sql={selected().sql}
                  {...(selected().kind === "inline_sql"
                    ? {
                      onSqlChange: (sql: string) =>
                        props.onInlineSql(sourceId(), sql),
                    }
                    : { readOnly: true })}
                  schema={props.sqlSchema}
                  actions={structureActions()}
                  variables={variables()}
                  parameterValues={parameterValues()}
                  parametersReadOnly
                  preview={{
                    status: status(),
                    isCurrent: state() === "ready",
                    ...(props.sourceCounts?.[sourceId()]
                      ? { count: props.sourceCounts[sourceId()] }
                      : {}),
                    ...(page()
                      ? {
                        page: {
                          columns: page()!.columns,
                          columnTypes: page()!.result_schema?.map((column) =>
                            column.type
                          ),
                          rows: page()!.rows,
                          exactIntegerRows: page()!.exact_integer_rows,
                          pageIdentity: `${sourceId()}:${
                            pageState()?.cursor ?? "first"
                          }`,
                          canNext: Boolean(page()!.has_more && page()!.next),
                          canPrevious:
                            (pageState()?.cursorStack.length ?? 0) > 1,
                        },
                      }
                      : {}),
                    ...(status() === "error"
                      ? { error: t("composition.queryFailed") }
                      : {}),
                  }}
                  onPreview={() => props.onRetry(sourceId())}
                  {...(props.onCount
                    ? { onCount: () => props.onCount?.(sourceId()) }
                    : {})}
                  onNext={() => props.onNext(sourceId())}
                  onPrevious={() => props.onPrevious(sourceId())}
                />
                <Show when={selected().kind === "inline_sql"}>
                  <SqlStructureToolbar
                    sql={() => selected().sql}
                    contexts={structureContexts}
                    onSqlChange={(sql) => props.onInlineSql(sourceId(), sql)}
                    onActionsChange={setStructureActions}
                  />
                </Show>
              </div>
            );
          }}
        </Show>
      </div>
    </div>
  );
}
