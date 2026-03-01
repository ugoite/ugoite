import { autocompletion } from "@codemirror/autocomplete";
import type { Diagnostic } from "@codemirror/lint";
import { linter, lintGutter } from "@codemirror/lint";
import { sql } from "@codemirror/lang-sql";
import { EditorState, Compartment } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { createEffect, onCleanup, onMount } from "solid-js";
import type { SqlSchema } from "~/lib/sql";
import { sqlLintDiagnostics } from "~/lib/sql";

export interface SqlQueryEditorProps {
	id?: string;
	value: string;
	onChange: (value: string) => void;
	schema: SqlSchema;
	onDiagnostics?: (diagnostics: Diagnostic[]) => void;
	disabled?: boolean;
}

export function SqlQueryEditor(props: SqlQueryEditorProps) {
	let host: HTMLDivElement | undefined;
	let view: EditorView | undefined;
	const schemaCompartment = new Compartment();
	const readonlyCompartment = new Compartment();
	const setHost = (el: HTMLDivElement) => {
		host = el;
	};

	onMount(() => {
		/* v8 ignore start */
		if (!host) return;
		/* v8 ignore stop */
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
				autocompletion(),
				lintGutter(),
				schemaCompartment.of(sql({ schema: props.schema })),
				readonlyCompartment.of(EditorState.readOnly.of(Boolean(props.disabled))),
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

		view = new EditorView({ state, parent: host });
		const diagnostics = sqlLintDiagnostics(view.state.doc.toString());
		/* v8 ignore start */
		props.onDiagnostics?.(diagnostics);
		/* v8 ignore stop */
	});

	createEffect(() => {
		/* v8 ignore start */
		if (!view) return;
		const nextValue = props.value;
		if (nextValue !== view.state.doc.toString()) {
			view.dispatch({
				changes: { from: 0, to: view.state.doc.length, insert: nextValue },
			});
		}
		/* v8 ignore stop */
	});

	createEffect(() => {
		/* v8 ignore start */
		if (!view) return;
		view.dispatch({
			effects: schemaCompartment.reconfigure(sql({ schema: props.schema })),
		});
		/* v8 ignore stop */
	});

	createEffect(() => {
		/* v8 ignore start */
		if (!view) return;
		view.dispatch({
			effects: readonlyCompartment.reconfigure(EditorState.readOnly.of(Boolean(props.disabled))),
		});
		/* v8 ignore stop */
	});

	onCleanup(() => {
		/* v8 ignore start */
		view?.destroy();
		/* v8 ignore stop */
	});

	return <div ref={setHost} id={props.id} class="ui-input ui-sql-editor text-sm" />;
	/* v8 ignore start */
}
/* v8 ignore stop */
