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

	onMount(() => {
		if (!host) return;
		const lintSource = (view: EditorView) => {
			const diagnostics = sqlLintDiagnostics(view.state.doc.toString());
			props.onDiagnostics?.(diagnostics);
			return diagnostics;
		};

		const state = EditorState.create({
			doc: props.value,
			extensions: [
				autocompletion(),
				lintGutter(),
				schemaCompartment.of(sql({ schema: props.schema })),
				readonlyCompartment.of(EditorState.readOnly.of(Boolean(props.disabled))),
				EditorView.updateListener.of((update) => {
					if (update.docChanged) {
						props.onChange(update.state.doc.toString());
					}
				}),
				linter(lintSource),
			],
		});

		view = new EditorView({ state, parent: host });
		lintSource(view);
	});

	createEffect(() => {
		if (!view) return;
		const nextValue = props.value;
		if (nextValue !== view.state.doc.toString()) {
			view.dispatch({
				changes: { from: 0, to: view.state.doc.length, insert: nextValue },
			});
		}
	});

	createEffect(() => {
		if (!view) return;
		view.dispatch({
			effects: schemaCompartment.reconfigure(sql({ schema: props.schema })),
		});
	});

	createEffect(() => {
		if (!view) return;
		view.dispatch({
			effects: readonlyCompartment.reconfigure(EditorState.readOnly.of(Boolean(props.disabled))),
		});
	});

	onCleanup(() => {
		view?.destroy();
	});

	return <div ref={host} id={props.id} class="border rounded-md text-sm" />;
}
