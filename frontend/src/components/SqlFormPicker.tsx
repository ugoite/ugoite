import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { t } from "~/lib/i18n";
import {
  applySqlDocumentSemanticEditTransaction,
  parseSqlDocument,
} from "~/lib/ugoite-client";
import type {
  SqlDocument,
  SqlDocumentNode,
  SqlDocumentSemanticEdit,
} from "~/lib/ugoite-client";

interface SqlFormPickerProps {
  sql: () => string;
  scope?: () => string;
  disabled?: () => boolean;
  onSqlChange: (sql: string) => void;
  forms: () => readonly { relation: string; label: string }[];
  onAdvanced: () => void;
  onApplied?: () => void;
}

/** A task-oriented view of Rust's document, never a second SQL parser or writer. */
export function SqlFormPicker(props: SqlFormPickerProps) {
  const [document, setDocument] = createSignal<SqlDocument>();
  const [loading, setLoading] = createSignal(true);
  const [busy, setBusy] = createSignal(false);
  const [targetId, setTargetId] = createSignal<number>();
  const [error, setError] = createSignal<string>();
  let generation = 0;
  onCleanup(() => {
    generation++;
  });

  createEffect(() => {
    const source = props.sql();
    props.scope?.();
    const request = ++generation;
    setLoading(true);
    setBusy(false);
    setDocument(undefined);
    setTargetId(undefined);
    setError(undefined);
    void parseSqlDocument(source).then((parsed) => {
      if (request !== generation || props.sql() !== source) return;
      setDocument(parsed);
      setLoading(false);
    }).catch(() => {
      if (request !== generation) return;
      setLoading(false);
      setError(t("sqlPage.structure.documentParseFailed"));
    });
  });

  const targets = createMemo(() => {
    const parsed = document();
    if (parsed?.status !== "parsed") return [];
    const nodes = new Map(parsed.nodes.map((node) => [node.id, node]));
    const owningSelect = (node: SqlDocumentNode) => {
      let parent = node.parent === null ? undefined : nodes.get(node.parent);
      while (parent && parent.kind !== "select") {
        parent = parent.parent === null ? undefined : nodes.get(parent.parent);
      }
      return parent?.id;
    };
    return parsed.nodes.filter((node) =>
      node.kind === "table_factor" ||
      (node.kind === "select" && !parsed.nodes.some((child) =>
        child.kind === "from_clause" && owningSelect(child) === node.id
      ))
    );
  });
  const target = () =>
    targets().find((node) => node.id === targetId()) ?? targets()[0];
  const relationName = (node: SqlDocumentNode | undefined) => {
    const relation = document()?.nodes.find((child) =>
      child.parent === node?.id && child.semantic?.kind === "relation"
    );
    return relation?.semantic?.kind === "relation"
      ? relation.semantic.name.map((part) => part.value).join(".")
      : undefined;
  };
  const targetLabel = (node: SqlDocumentNode, index: number) => {
    const name = relationName(node);
    const label = props.forms().find((form) => form.relation === name)?.label ??
      name ??
      t(
        node.kind === "select"
          ? "sqlPage.structure.noForm"
          : "sqlPage.structure.nestedSource",
      );
    const alias = node.semantic?.kind === "table_factor"
      ? node.semantic.alias?.value
      : undefined;
    return `${index + 1}. ${alias ? `${alias} · ` : ""}${label}`;
  };

  const selectForm = async (relation: string) => {
    const parsed = document();
    const form = props.forms().find((item) => item.relation === relation);
    if (!parsed || !form || busy() || props.disabled?.()) return;
    const selected = target();
    const name = { value: form.relation, quote_style: '"' };
    const factor = {
      kind: "table" as const,
      name: [name],
      alias: selected?.semantic?.kind === "table_factor"
        ? selected.semantic.alias
        : null,
    };
    const edit: SqlDocumentSemanticEdit | undefined = parsed.status === "empty"
      ? { kind: "initialize_select", relation: name }
      : selected?.kind === "table_factor"
      ? {
        kind: "replace_table_factor",
        node_id: selected.id,
        expected_kind: "table_factor",
        factor,
      }
      : selected?.kind === "select"
      ? {
        kind: "add_from_relation",
        node_id: selected.id,
        expected_kind: "select",
        factor,
      }
      : undefined;
    if (!edit) return;
    const expectedSource = parsed.source;
    const expectedScope = props.scope?.();
    const request = ++generation;
    setBusy(true);
    setError(undefined);
    try {
      const result = await applySqlDocumentSemanticEditTransaction(
        expectedSource,
        {
          contract_version: parsed.contract_version,
          expected_source: expectedSource,
          edits: [edit],
        },
      );
      if (
        request !== generation || props.sql() !== expectedSource ||
        props.scope?.() !== expectedScope || props.disabled?.()
      ) return;
      if (result.document.status !== "parsed") {
        throw new Error("invalid result");
      }
      props.onSqlChange(result.document.source);
      props.onApplied?.();
    } catch {
      if (request === generation) {
        setError(t("sqlPage.structure.documentEditUnavailable"));
      }
    } finally {
      if (request === generation) setBusy(false);
    }
  };

  const available = () =>
    document()?.status === "empty" || targets().length > 0;
  return (
    <div class="sql-form-picker">
      <Show when={loading()}>
        <p role="status">{t("sqlPage.structure.documentLoading")}</p>
      </Show>
      <Show when={targets().length > 1}>
        <label class="ui-field">
          <span>{t("sqlPage.structure.sourceToChange")}</span>
          <select
            class="ui-input"
            value={target()?.id}
            disabled={busy() || props.disabled?.()}
            onChange={(event) => setTargetId(Number(event.currentTarget.value))}
          >
            <For each={targets()}>
              {(node, index) => (
                <option value={node.id}>{targetLabel(node, index())}</option>
              )}
            </For>
          </select>
        </label>
      </Show>
      <Show when={!loading() && !available()}>
        <p role="status">{t("sqlPage.structure.fixSqlBeforeForm")}</p>
      </Show>
      <div
        class="sql-form-picker__list"
        role="group"
        aria-label={t("sqlPage.structure.availableForms")}
      >
        <For each={props.forms().map((form) => form.relation)}>
          {(relation) => (
            <button
              type="button"
              class="sql-form-picker__option ui-button ui-button-secondary"
              aria-pressed={relationName(target()) === relation}
              aria-disabled={busy() || undefined}
              disabled={!available() || props.disabled?.()}
              onClick={() => void selectForm(relation)}
            >
              {props.forms().find((form) => form.relation === relation)?.label}
            </button>
          )}
        </For>
      </div>
      <Show when={error()}>
        <p role="alert" class="ui-text-danger">{error()}</p>
      </Show>
      <button
        type="button"
        class="ui-button ui-button-ghost"
        disabled={busy()}
        onClick={props.onAdvanced}
      >
        {t("sqlPage.structure.advancedStructure")}
      </button>
    </div>
  );
}
