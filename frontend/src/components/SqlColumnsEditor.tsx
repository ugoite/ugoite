import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
  untrack,
} from "solid-js";
import { IconButton } from "~/components/IconButton";
import { t } from "~/lib/i18n";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import {
  type PlannedSqlColumns,
  type PlanSqlColumns,
  usablePlannedColumnNames,
  wildcardProjectionQualifier,
} from "~/lib/sql-document-columns";
import {
  applySqlDocumentEditTransaction,
  applySqlDocumentSemanticEditTransaction,
  parseSqlDocument,
  UgoiteApiError,
} from "~/lib/ugoite-client";
import type {
  SqlDocument,
  SqlDocumentColumnReference,
  SqlDocumentEditTransaction,
  SqlDocumentNode,
  SqlDocumentSemanticEdit,
} from "~/lib/ugoite-client";

interface SqlColumnsEditorProps {
  sql: () => string;
  scope?: () => string;
  contexts: () => readonly SqlStructureAdapterContext[];
  disabled?: () => boolean;
  onSqlChange: (sql: string) => void;
  onAdvanced: () => void;
  onPlanColumns?: PlanSqlColumns;
}

/** A projection view of Rust's document. Identifiers and edits stay typed. */
export function SqlColumnsEditor(props: SqlColumnsEditorProps) {
  const [document, setDocument] = createSignal<SqlDocument>();
  const [loading, setLoading] = createSignal(true);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [selectIndex, setSelectIndex] = createSignal(0);
  const [columnId, setColumnId] = createSignal("");
  const [history, setHistory] = createSignal<SqlDocumentEditTransaction[]>([]);
  const [redoHistory, setRedoHistory] = createSignal<
    SqlDocumentEditTransaction[]
  >([]);
  let root: HTMLDivElement | undefined;
  let pendingFocus: {
    source: string;
    scope: string | undefined;
    element: HTMLElement;
  } | undefined;
  let generation = 0;
  let previousScope = props.scope?.();
  onCleanup(() => generation++);
  createEffect(() => {
    const source = props.sql();
    const scope = props.scope?.();
    const request = ++generation;
    if (scope !== previousScope) {
      previousScope = scope;
      pendingFocus = undefined;
      setSelectIndex(0);
      setColumnId("");
      setHistory([]);
      setRedoHistory([]);
    } else if (
      untrack(history).at(-1)?.expected_source !== source &&
      untrack(redoHistory).at(-1)?.expected_source !== source
    ) {
      setHistory([]);
      setRedoHistory([]);
    }
    setLoading(true);
    setBusy(false);
    setError(undefined);
    void parseSqlDocument(source).then((parsed) => {
      if (
        request !== generation || props.sql() !== source ||
        props.scope?.() !== scope
      ) return;
      setDocument(parsed);
      setLoading(false);
      const focus = pendingFocus;
      pendingFocus = undefined;
      if (
        focus?.source === source && focus.scope === scope &&
        globalThis.document.activeElement === globalThis.document.body
      ) {
        const target = focus.element.isConnected
          ? focus.element
          : root?.querySelector<HTMLElement>(
            'button:not([disabled]):not([aria-disabled="true"]), select:not([disabled])',
          );
        target?.focus();
      }
    }).catch(() => {
      if (request !== generation) return;
      setDocument(undefined);
      setLoading(false);
      setError(t("sqlPage.structure.documentParseFailed"));
    });
  });
  const locked = () => busy() || loading() || props.disabled?.();
  const nodes = () => document()?.nodes ?? [];
  const byId = createMemo(() =>
    new Map(nodes().map((node) => [node.id, node]))
  );
  const children = (node: SqlDocumentNode | undefined) =>
    node ? nodes().filter((child) => child.parent === node.id) : [];
  const ancestor = (node: SqlDocumentNode, kind: SqlDocumentNode["kind"]) => {
    let current: SqlDocumentNode | undefined = node;
    while (current && current.kind !== kind) {
      current = current.parent === null
        ? undefined
        : byId().get(current.parent);
    }
    return current;
  };
  const selects = () => nodes().filter((node) => node.kind === "select");
  const select = () => selects()[selectIndex()] ?? selects()[0];
  const list = () =>
    children(select()).find((node) => node.kind === "select_list");
  const items = () =>
    children(list()).filter((node) => node.kind === "select_item");
  const sourceSlice = (node: SqlDocumentNode) =>
    node.range
      ? new TextDecoder().decode(
        new TextEncoder().encode(document()?.source ?? "").slice(
          node.range.start,
          node.range.end,
        ),
      )
      : "";
  const bindings = createMemo(() => {
    const selected = select();
    if (
      !selected ||
      nodes().some((node) => node.kind === "common_table_expression")
    ) return [];
    return nodes().flatMap((factor) => {
      if (
        factor.semantic?.kind !== "table_factor" ||
        factor.semantic.factor_kind !== "table" ||
        ancestor(factor, "select")?.id !== selected.id
      ) return [];
      const relation = children(factor).find((node) =>
        node.semantic?.kind === "relation"
      );
      if (
        relation?.semantic?.kind !== "relation" ||
        relation.semantic.name.length !== 1
      ) return [];
      const name = relation.semantic.name[0];
      const context = props.contexts().find((candidate) =>
        candidate.parser.relation === name.value
      );
      if (!context) return [];
      return [{ context, qualifier: factor.semantic.alias ?? name }];
    });
  });
  const fieldLabel = (
    context: SqlStructureAdapterContext | undefined,
    name: string,
  ) =>
    context?.columnLabels?.[name] ??
      (name === "_ugoite_id"
        ? t("entryInfo.entryId")
        : name === "_ugoite_created_at"
        ? t("entryInfo.createdAt")
        : name === "_ugoite_updated_at"
        ? t("entryInfo.updatedAt")
        : name);
  const choices = createMemo(() =>
    bindings().flatMap(({ context, qualifier }, bindingIndex) =>
      context.parser.columns.flatMap((column) => {
        if (context.columnCapabilities[column.name]?.projectable !== true) {
          return [];
        }
        return [{
          id: `${bindingIndex}:${column.name}`,
          label: fieldLabel(context, column.name),
          formLabel: qualifier.value === context.parser.relation
            ? context.label
            : qualifier.value,
          reference: {
            qualifier,
            column: { value: column.name, quote_style: '"' },
          } satisfies SqlDocumentColumnReference,
        }];
      })
    )
  );
  const choice = () =>
    choices().find((item) => item.id === columnId()) ?? choices()[0];
  const wildcardQualifier = (item: SqlDocumentNode) => {
    const parsed = document();
    return parsed && props.onPlanColumns
      ? wildcardProjectionQualifier(
        parsed,
        item,
        props.contexts().map((context) => context.parser.relation),
      )
      : undefined;
  };
  const itemLabel = (item: SqlDocumentNode) => {
    if (item.semantic?.kind === "wildcard" && !item.semantic.has_options) {
      const qualifier = item.semantic.qualifier.at(-1)?.value;
      return qualifier
        ? `${qualifier} · ${t("sqlPage.columns.allFields")}`
        : t("sqlPage.columns.allFields");
    }
    const expression = children(item)[0];
    const semantic = expression?.semantic;
    const reference = semantic?.kind === "identifier"
      ? [semantic.value]
      : semantic?.kind === "compound_identifier"
      ? semantic.parts.map((part) => part.value)
      : [];
    const matching = choices().filter((candidate) =>
      candidate.reference.column.value === reference.at(-1) &&
      (reference.length === 1 ||
        candidate.reference.qualifier.value === reference.at(-2))
    );
    if (reference.length > 0 && sourceSlice(item) === sourceSlice(expression)) {
      return matching.length === 1
        ? matching[0].label
        : fieldLabel(undefined, reference.at(-1)!);
    }
    return sourceSlice(item);
  };
  // Work-only keys keep a field's controls attached while its output order changes.
  const rows = createMemo(() => {
    const occurrences = new Map<string, number>();
    return items().map((node) => {
      const text = sourceSlice(node);
      const ordinal = occurrences.get(text) ?? 0;
      occurrences.set(text, ordinal + 1);
      return { key: JSON.stringify([text, ordinal]), node };
    });
  });
  const apply = async (
    edits: SqlDocumentSemanticEdit[] | undefined,
    direction?: "undo" | "redo",
    opener?: HTMLElement,
  ) => {
    const parsed = document();
    const previous = (direction === "redo" ? redoHistory() : history()).at(-1);
    if (
      !parsed || parsed.status !== "parsed" || locked() ||
      (direction && !previous)
    ) return;
    const expectedSource = parsed.source;
    const expectedScope = props.scope?.();
    const request = ++generation;
    setBusy(true);
    setError(undefined);
    try {
      let columnPlan: PlannedSqlColumns | undefined;
      if (edits?.[0]?.kind === "expand_projection_wildcard") {
        const edit = edits[0];
        const item = parsed.nodes.find((node) => node.id === edit.node_id);
        const qualifier = item && wildcardQualifier(item);
        if (!qualifier || !props.onPlanColumns) {
          throw new Error("unavailable wildcard schema");
        }
        columnPlan = await props.onPlanColumns();
        if (
          !(await columnPlan.isCurrent()) ||
          !usablePlannedColumnNames(columnPlan.columns)
        ) throw new Error("stale or ambiguous wildcard schema");
        if (
          request !== generation || props.sql() !== expectedSource ||
          props.scope?.() !== expectedScope || props.disabled?.()
        ) return;
        edits = [{
          ...edit,
          columns: columnPlan.columns.map((name) => ({
            qualifier,
            column: { value: name, quote_style: '"' },
          })),
        }];
      }
      const result = direction
        ? await applySqlDocumentEditTransaction(expectedSource, previous!)
        : await applySqlDocumentSemanticEditTransaction(expectedSource, {
          contract_version: parsed.contract_version,
          expected_source: expectedSource,
          edits: edits!,
        });
      if (columnPlan && !(await columnPlan.isCurrent())) return;
      if (
        request !== generation || props.sql() !== expectedSource ||
        props.scope?.() !== expectedScope || props.disabled?.()
      ) return;
      if (result.document.status !== "parsed") {
        throw new Error("invalid result");
      }
      if (direction === "undo") {
        setHistory(history().slice(0, -1));
        setRedoHistory([...redoHistory(), result.undo].slice(-32));
      } else {
        setHistory([...history(), result.undo].slice(-32));
        setRedoHistory(direction === "redo" ? redoHistory().slice(0, -1) : []);
      }
      const active = globalThis.document.activeElement;
      const focusTarget =
        active instanceof HTMLElement && active !== globalThis.document.body
          ? active
          : opener;
      pendingFocus = focusTarget
        ? {
          source: result.document.source,
          scope: expectedScope,
          element: focusTarget,
        }
        : undefined;
      props.onSqlChange(result.document.source);
    } catch (cause) {
      if (request === generation) {
        setError(
          t(
            cause instanceof UgoiteApiError &&
              cause.code === "projection_position_conflict"
                ? "sqlPage.columns.positionConflict"
                : cause.code === "projection_alias_conflict"
                  ? "sqlPage.columns.aliasConflict"
              : "sqlPage.structure.documentEditUnavailable",
          ),
        );
      }
    } finally {
      if (request === generation) setBusy(false);
    }
  };
  return (
    <div ref={root} class="sql-columns-editor" aria-busy={loading() || busy()}>
      <Show when={loading()}>
        <p role="status">{t("sqlPage.structure.documentLoading")}</p>
      </Show>
      <Show when={!loading() && document()?.status !== "parsed"}>
        <p role="status">{t("sqlPage.columns.fixSql")}</p>
      </Show>
      <Show when={document()?.status === "parsed"}>
        <Show when={selects().length > 1}>
          <label class="ui-field">
            <span>{t("sqlPage.columns.resultToChange")}</span>
            <select
              class="ui-input"
              value={selectIndex()}
              disabled={locked()}
              onChange={(event) => {
                setSelectIndex(Number(event.currentTarget.value));
                setColumnId("");
              }}
            >
              <For each={selects()}>
                {(_, index) => (
                  <option value={index()}>
                    {t("sqlPage.columns.resultNumber", { number: index() + 1 })}
                  </option>
                )}
              </For>
            </select>
          </label>
        </Show>
        <ol
          class="sql-columns-editor__list"
          aria-label={t("sqlPage.columns.resultFields")}
        >
          <For
            each={rows().map((row) =>
              row.key
            )}
          >
            {(key, index) => {
              const item = () =>
                rows().find((row) => row.key === key)!.node;
              return (
                <li class="sql-columns-editor__row">
                  <span class="sql-columns-editor__name">
                    {itemLabel(item())}
                  </span>
                  <Show
                    when={item().semantic?.kind === "wildcard" &&
                      !wildcardQualifier(item())}
                  >
                    <span role="status" class="ui-text-muted">
                      {t("sqlPage.columns.expansionUnavailable")}
                    </span>
                  </Show>
                  <div class="sql-columns-editor__row-actions">
                    <Show
                      when={item().semantic?.kind === "wildcard" &&
                        wildcardQualifier(item())}
                    >
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        aria-disabled={locked() || undefined}
                        onClick={(event) =>
                          void apply(
                            [{
                              kind: "expand_projection_wildcard",
                              node_id: item().id,
                              expected_kind: "select_item",
                              columns: [],
                            }],
                            undefined,
                            event.currentTarget,
                          )}
                      >
                        {t("sqlPage.columns.chooseFields")}
                      </button>
                    </Show>
                    <IconButton
                      icon="arrow-up"
                      label={t("sqlPage.columns.moveUp", {
                        name: itemLabel(item()),
                      })}
                      ariaDisabled={index() === 0 || locked()}
                      onClick={(event) =>
                        void apply(
                          [{
                            kind: "move_projection_item",
                            node_id: item().id,
                            expected_kind: "select_item",
                            to_index: index() - 1,
                          }],
                          undefined,
                          event.currentTarget,
                        )}
                    />
                    <IconButton
                      icon="arrow-down"
                      label={t("sqlPage.columns.moveDown", {
                        name: itemLabel(item()),
                      })}
                      ariaDisabled={index() === items().length - 1 || locked()}
                      onClick={(event) =>
                        void apply(
                          [{
                            kind: "move_projection_item",
                            node_id: item().id,
                            expected_kind: "select_item",
                            to_index: index() + 1,
                          }],
                          undefined,
                          event.currentTarget,
                        )}
                    />
                    <IconButton
                      icon="trash"
                      label={t("sqlPage.columns.remove", {
                        name: itemLabel(item()),
                      })}
                      ariaDisabled={items().length === 1 || locked()}
                      onClick={(event) =>
                        void apply(
                          [{
                            kind: "remove_projection_item",
                            node_id: item().id,
                            expected_kind: "select_item",
                          }],
                          undefined,
                          event.currentTarget,
                        )}
                    />
                  </div>
                </li>
              );
            }}
          </For>
        </ol>
        <Show
          when={choices().length > 0 && list() &&
            !items().some((item) => item.semantic?.kind === "wildcard")}
        >
          <div class="sql-columns-editor__add">
            <label class="ui-field">
              <span>{t("sqlPage.columns.addField")}</span>
              <select
                class="ui-input"
                value={choice()?.id}
                disabled={locked()}
                onChange={(event) => setColumnId(event.currentTarget.value)}
              >
                <For each={choices().map((item) => item.id)}>
                  {(id) => (
                    <option value={id}>
                      {(() => {
                        const item = choices().find((choice) =>
                          choice.id === id
                        );
                        return bindings().length > 1
                          ? `${item?.formLabel} · ${item?.label}`
                          : item?.label;
                      })()}
                    </option>
                  )}
                </For>
              </select>
            </label>
            <button
              type="button"
              class="ui-button ui-button-secondary"
              aria-disabled={locked() || !choice() || undefined}
              onClick={(event) =>
                void apply(
                  [{
                    kind: "add_projection_column",
                    node_id: list()!.id,
                    expected_kind: "select_list",
                    column: choice()!.reference,
                  }],
                  undefined,
                  event.currentTarget,
                )}
            >
              {t("common.add")}
            </button>
          </div>
        </Show>
      </Show>
      <Show when={error()}>
        <p role="alert" class="ui-text-danger">{error()}</p>
      </Show>
      <div class="sql-columns-editor__footer">
        <button
          type="button"
          class="ui-button ui-button-ghost"
          aria-disabled={locked() || history().length === 0 || undefined}
          onClick={(event) =>
            void apply(undefined, "undo", event.currentTarget)}
        >
          {t("sqlPage.columns.undo")}
        </button>
        <button
          type="button"
          class="ui-button ui-button-ghost"
          aria-disabled={locked() || redoHistory().length === 0 || undefined}
          onClick={(event) =>
            void apply(undefined, "redo", event.currentTarget)}
        >
          {t("sqlPage.columns.redo")}
        </button>
        <button
          type="button"
          class="ui-button ui-button-ghost"
          aria-disabled={locked() || undefined}
          onClick={() => {
            if (!locked()) props.onAdvanced();
          }}
        >
          {t("sqlPage.structure.advancedStructure")}
        </button>
      </div>
    </div>
  );
}
