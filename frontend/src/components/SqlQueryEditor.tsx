import type { Diagnostic } from "@codemirror/lint";
import { history, historyKeymap, isolateHistory } from "@codemirror/commands";
import { forceLinting, linter, lintGutter } from "@codemirror/lint";
import { sql } from "@codemirror/lang-sql";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { createEffect, createSignal, onCleanup } from "solid-js";
import type { SqlSchema } from "~/lib/sql";
import { sqlLintDiagnostics } from "~/lib/sql";
import { sqlEditorAutocompletion } from "~/lib/sql-completion";

export interface SqlQueryEditorProps {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  schema: SqlSchema;
  onDiagnostics?: (diagnostics: Diagnostic[]) => void;
  additionalDiagnostics?: readonly Diagnostic[];
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
    const lintSource: Parameters<typeof linter>[0] = async (view) => {
      const source = view.state.doc.toString();
      const diagnostics = [
        ...await sqlLintDiagnostics(source),
        ...(props.additionalDiagnostics ?? []),
      ];
      if (view.state.doc.toString() === source) {
        props.onDiagnostics?.(diagnostics);
      }
      return diagnostics;
    };
    /* v8 ignore stop */

    const state = EditorState.create({
      doc: props.value,
      extensions: [
        history(),
        keymap.of(historyKeymap),
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
            props.onChange(update.state.doc.toString());
          }
          /* v8 ignore stop */
        }),
        linter(lintSource),
      ],
    });

    const editorView = new EditorView({ state, parent: container });
    setView(editorView);
    /* v8 ignore start */
    const source = editorView.state.doc.toString();
    void sqlLintDiagnostics(source).then((diagnostics) => {
      if (editorView.state.doc.toString() === source) {
        props.onDiagnostics?.([
          ...diagnostics,
          ...(props.additionalDiagnostics ?? []),
        ]);
      }
    });
    /* v8 ignore stop */
  };

  createEffect(() => {
    /* v8 ignore start */
    const editorView = view();
    const diagnostics = props.additionalDiagnostics;
    if (editorView && diagnostics !== undefined) forceLinting(editorView);
    /* v8 ignore stop */
  });

  createEffect(() => {
    /* v8 ignore start */
    const editorView = view();
    if (!editorView) return;
    const nextValue = props.value;
    if (nextValue !== editorView.state.doc.toString()) {
      editorView.dispatch({
        annotations: isolateHistory.of("full"),
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
