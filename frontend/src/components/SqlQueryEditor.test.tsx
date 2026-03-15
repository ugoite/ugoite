import { createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import { render, waitFor } from "@solidjs/testing-library";
import { SqlQueryEditor } from "./SqlQueryEditor";
import { buildSqlSchema } from "~/lib/sql";

vi.mock("@codemirror/autocomplete", () => ({
	autocompletion: () => ({ type: "autocompletion" }),
}));

vi.mock("@codemirror/lint", () => ({
	lintGutter: () => ({ type: "lintGutter" }),
	linter: (source: unknown) => ({ type: "linter", source }),
}));

vi.mock("@codemirror/lang-sql", () => ({
	sql: (config: unknown) => ({ type: "sql", config }),
}));

vi.mock("@codemirror/state", () => {
	class FakeCompartment {
		of(extension: unknown) {
			return extension;
		}

		reconfigure(extension: unknown) {
			return extension;
		}
	}

	return {
		Compartment: FakeCompartment,
		EditorState: {
			create: ({ doc, extensions = [] }: { doc: string; extensions?: unknown[] }) => ({
				doc: {
					toString: () => doc,
					get length() {
						return doc.length;
					},
				},
				extensions,
			}),
			readOnly: {
				of: (value: boolean) => ({ type: "readOnly", value }),
			},
		},
	};
});

vi.mock("@codemirror/view", () => {
	const updateListener = {
		of: (listener: unknown) => ({ type: "updateListener", listener }),
	};

	return {
		EditorView: class FakeEditorView {
			static updateListener = updateListener;

			state: {
				doc: {
					toString: () => string;
					length: number;
				};
				extensions?: unknown[];
			};
			listeners: Array<(update: { docChanged: boolean; state: typeof this.state }) => void>;

			constructor({
				state,
			}: {
				state: { doc: { toString: () => string; length: number }; extensions?: unknown[] };
			}) {
				this.state = state;
				this.listeners = (state.extensions ?? [])
					.filter(
						(
							extension,
						): extension is { type: "updateListener"; listener: (typeof this.listeners)[number] } =>
							typeof extension === "object" &&
							extension !== null &&
							"type" in extension &&
							extension.type === "updateListener" &&
							"listener" in extension &&
							typeof extension.listener === "function",
					)
					.map((extension) => extension.listener);

				for (const extension of state.extensions ?? []) {
					if (
						typeof extension === "object" &&
						extension !== null &&
						"type" in extension &&
						extension.type === "linter" &&
						"source" in extension &&
						typeof extension.source === "function"
					) {
						extension.source({ state: this.state });
					}
				}
			}

			dispatch({
				changes,
			}: {
				changes?: { from: number; to: number; insert: string };
				effects?: unknown;
			}) {
				if (changes) {
					const nextValue = changes.insert;
					this.state = {
						doc: {
							toString: () => nextValue,
							length: nextValue.length,
						},
						extensions: this.state.extensions,
					};
					for (const listener of this.listeners) {
						listener({ docChanged: true, state: this.state });
					}
				}
			}

			destroy() {
				return undefined;
			}
		},
	};
});

describe("REQ-FE-036: SQL Query Editor", () => {
	it("should render without runtime errors", () => {
		const onDiagnostics = vi.fn();
		const result = render(() => (
			<SqlQueryEditor
				value="SELECT * FROM entries"
				onChange={() => undefined}
				schema={buildSqlSchema([])}
				onDiagnostics={onDiagnostics}
			/>
		));
		expect(onDiagnostics).toHaveBeenCalled();
		result.unmount();
	});

	it("propagates editor updates through the change listener", async () => {
		const [value, setValue] = createSignal("SELECT * FROM entries");
		const onChange = vi.fn();

		const result = render(() => (
			<SqlQueryEditor value={value()} onChange={onChange} schema={buildSqlSchema([])} />
		));

		setValue("SELECT id FROM entries");

		await waitFor(() => {
			expect(onChange).toHaveBeenCalledWith("SELECT id FROM entries");
		});

		result.unmount();
	});
});
