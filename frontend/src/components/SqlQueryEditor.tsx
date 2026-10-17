import type { Diagnostic } from "@codemirror/lint";
import { linter, lintGutter } from "@codemirror/lint";
import { sql } from "@codemirror/lang-sql";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { createEffect, createSignal, onCleanup } from "solid-js";
import type { SqlSchema } from "~/lib/sql";
import { sqlLintDiagnostics } from "~/lib/sql";
import { sqlEditorAutocompletion } from "~/lib/sql-completion";

export interface SqlQueryEditorProps {
  id?: string;
  value: string;
  onChange: (value: string, userInitiated: boolean) => void;
  schema: SqlSchema;
  onDiagnostics?: (diagnostics: Diagnostic[]) => void;
  ariaLabel?: string;
  wrapLines?: boolean;
  disabled?: boolean;
}

export function SqlQueryEditor(props: SqlQueryEditorProps) {
  const [view, setView] = createSignal<EditorView | undefined>(undefined);
  const schemaCompartment = new Compartment();
  const contentAttributesCompartment = new Compartment();
  const wrappingCompartment = new Compartment();
  const readonlyCompartment = new Compartment();
  const editableCompartment = new Compartment();

  const initEditor = (container: HTMLDivElement) => {
    if (view()) return;
    /* v8 ignore start */
    const lintSource: Parameters<typeof linter>[0] = (view) => {
      const diagnostics = sqlLintDiagnostics(view.state.doc.toString());
      props.onDiagnostics?.(diagnostics);
      return diagnostics;
    };
    /* v8 ignore stop */

    const state = EditorState.create({
      doc: props.value,
      extensions: [
        lintGutter(),
        schemaCompartment.of([
          sql({ schema: props.schema }),
          sqlEditorAutocompletion(props.schema),
        ]),
        contentAttributesCompartment.of(
          EditorView.contentAttributes.of(
            props.ariaLabel ? { "aria-label": props.ariaLabel } : {},
          ),
        ),
        wrappingCompartment.of(
          props.wrapLines ? EditorView.lineWrapping : [],
        ),
        readonlyCompartment.of(
          EditorState.readOnly.of(Boolean(props.disabled)),
        ),
        editableCompartment.of(
          EditorView.editable.of(!props.disabled),
        ),
        EditorView.updateListener.of((update) => {
          /* v8 ignore start */
          if (update.docChanged) {
            const userInitiated = update.transactions.some((transaction) =>
              transaction.isUserEvent("input") ||
              transaction.isUserEvent("delete") ||
              transaction.isUserEvent("move") ||
              transaction.isUserEvent("undo") ||
              transaction.isUserEvent("redo")
            );
            props.onChange(update.state.doc.toString(), userInitiated);
          }
          /* v8 ignore stop */
        }),
        linter(lintSource),
      ],
    });

    const editorView = new EditorView({ state, parent: container });
    setView(editorView);
    const diagnostics = sqlLintDiagnostics(editorView.state.doc.toString());
    /* v8 ignore start */
    props.onDiagnostics?.(diagnostics);
    /* v8 ignore stop */
  };

  createEffect(() => {
    /* v8 ignore start */
    const editorView = view();
    if (!editorView) return;
    const nextValue = props.value;
    if (nextValue !== editorView.state.doc.toString()) {
      editorView.dispatch({
        changes: {
          from: 0,
          to: editorView.state.doc.length,
          insert: nextValue,
        },
      });
    }
    /* v8 ignore stop */
  });

  createEffect(() => {
    /* v8 ignore start */
    const editorView = view();
    if (!editorView) return;
    editorView.dispatch({
      effects: schemaCompartment.reconfigure([
        sql({ schema: props.schema }),
        sqlEditorAutocompletion(props.schema),
      ]),
    });
    /* v8 ignore stop */
  });

  createEffect(() => {
    /* v8 ignore start */
    const editorView = view();
    if (!editorView) return;
    editorView.dispatch({
      effects: editableCompartment.reconfigure(
        EditorView.editable.of(!props.disabled),
      ),
    });
    /* v8 ignore stop */
  });

  createEffect(() => {
    /* v8 ignore start */
    const editorView = view();
    if (!editorView) return;
    editorView.dispatch({
      effects: contentAttributesCompartment.reconfigure(
        EditorView.contentAttributes.of(
          props.ariaLabel ? { "aria-label": props.ariaLabel } : {},
        ),
      ),
    });
    /* v8 ignore stop */
  });

  createEffect(() => {
    /* v8 ignore start */
    const editorView = view();
    if (!editorView) return;
    editorView.dispatch({
      effects: wrappingCompartment.reconfigure(
        props.wrapLines ? EditorView.lineWrapping : [],
      ),
    });
    /* v8 ignore stop */
  });

  createEffect(() => {
    /* v8 ignore start */
    const editorView = view();
    if (!editorView) return;
    editorView.dispatch({
      effects: readonlyCompartment.reconfigure(
        EditorState.readOnly.of(Boolean(props.disabled)),
      ),
    });
    /* v8 ignore stop */
  });

  onCleanup(() => {
    /* v8 ignore start */
    view()?.destroy();
    /* v8 ignore stop */
  });

  return (
    <div
      ref={(el) => {
        initEditor(el);
      }}
      id={props.id}
      class="ui-input ui-sql-editor text-sm"
    />
  );
  /* v8 ignore start */
}
/* v8 ignore stop */
