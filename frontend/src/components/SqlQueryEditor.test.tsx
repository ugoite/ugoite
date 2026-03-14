import { describe, expect, it, vi } from "vitest";
import { render } from "@solidjs/testing-library";
import { SqlQueryEditor } from "./SqlQueryEditor";
import { buildSqlSchema } from "~/lib/sql";

vi.mock("@codemirror/autocomplete", () => ({
	autocompletion: () => ({ type: "autocompletion" }),
}));

vi.mock("@codemirror/lint", () => ({
	lintGutter: () => ({ type: "lintGutter" }),
	linter: () => ({ type: "linter" }),
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
			create: ({ doc }: { doc: string }) => ({
				doc: {
					toString: () => doc,
					get length() {
						return doc.length;
					},
				},
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
			};

			constructor({ state }: { state: { doc: { toString: () => string; length: number } } }) {
				this.state = state;
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
					};
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
});
