import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import { render, waitFor } from "@solidjs/testing-library";
import { SqlQueryEditor } from "./SqlQueryEditor";
import { buildSqlSchema } from "~/lib/sql";

const codeMirrorViewMocks = vi.hoisted(() => ({
  contentAttributesOf: vi.fn((attributes: Record<string, string>) => ({
    type: "contentAttributes",
    attributes,
  })),
  editableOf: vi.fn((value: boolean) => ({ type: "editable", value })),
  lineWrapping: { type: "lineWrapping" },
}));

vi.mock("@codemirror/autocomplete", () => ({
  autocompletion: (config: unknown) => ({ type: "autocompletion", config }),
}));

vi.mock("@codemirror/language", () => ({
  syntaxTree: () => ({ resolve: () => ({ name: "Identifier" }) }),
}));

vi.mock("@codemirror/lint", () => ({
  lintGutter: () => ({ type: "lintGutter" }),
  linter: (source: unknown) => ({ type: "linter", source }),
}));

vi.mock("@codemirror/lang-sql", () => ({
  sql: (config: unknown) => ({ type: "sql", config }),
  schemaCompletionSource: () => () => null,
  keywordCompletionSource: () => () => null,
  StandardSQL: {
    language: {
      parser: {
        parse: () => ({
          cursor: () => ({
            firstChild: () => false,
          }),
        }),
      },
    },
  },
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
      create: (
        { doc, extensions = [] }: { doc: string; extensions?: unknown[] },
      ) => ({
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
      static contentAttributes = {
        of: codeMirrorViewMocks.contentAttributesOf,
      };
      static editable = { of: codeMirrorViewMocks.editableOf };
      static lineWrapping = codeMirrorViewMocks.lineWrapping;

      state: {
        doc: {
          toString: () => string;
          length: number;
        };
        extensions?: unknown[];
      };
      listeners: Array<
        (update: { docChanged: boolean; state: typeof this.state }) => void
      >;

      constructor({
        state,
        parent,
      }: {
        state: {
          doc: { toString: () => string; length: number };
          extensions?: unknown[];
        };
        parent?: HTMLDivElement;
      }) {
        this.state = state;
        const contentAttributes = (state.extensions ?? []).find(
          (extension) =>
            typeof extension === "object" && extension !== null &&
            "type" in extension && extension.type === "contentAttributes" &&
            "attributes" in extension &&
            typeof extension.attributes === "object",
        );
        if (parent) {
          const label = contentAttributes && "attributes" in contentAttributes
            ? (contentAttributes.attributes as Record<string, string>)[
              "aria-label"
            ]
            : undefined;
          parent.dataset.editorAccessibleName = label ?? "";
          parent.dataset.lineWrapping = String(
            (state.extensions ?? []).includes(codeMirrorViewMocks.lineWrapping),
          );
          const editable = (state.extensions ?? []).find(
            (extension) =>
              typeof extension === "object" && extension !== null &&
              "type" in extension && extension.type === "editable" &&
              "value" in extension,
          );
          parent.dataset.editorEditable = String(
            editable && "value" in editable ? editable.value : "",
          );
        }
        this.listeners = (state.extensions ?? [])
          .filter(
            (
              extension,
            ): extension is {
              type: "updateListener";
              listener: (typeof this.listeners)[number];
            } =>
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
        value='SELECT * FROM "form_00000000000000000000000000000001"'
        onChange={() => undefined}
        schema={buildSqlSchema([])}
        onDiagnostics={onDiagnostics}
      />
    ));
    expect(onDiagnostics).toHaveBeenCalled();
    expect(result.container.querySelector(".ui-sql-editor")).toHaveAttribute(
      "data-line-wrapping",
      "false",
    );
    result.unmount();
  });

  it("names the SQL editor and enables line wrapping when requested", () => {
    codeMirrorViewMocks.contentAttributesOf.mockClear();
    const result = render(() => (
      <SqlQueryEditor
        value='SELECT * FROM "form_00000000000000000000000000000001"'
        onChange={() => undefined}
        schema={buildSqlSchema([])}
        ariaLabel="SQL"
        wrapLines
      />
    ));

    expect(codeMirrorViewMocks.contentAttributesOf)
      .toHaveBeenCalledWith({ "aria-label": "SQL" });
    expect(result.container.querySelector(".ui-sql-editor")).toHaveAttribute(
      "data-editor-accessible-name",
      "SQL",
    );
    expect(result.container.querySelector(".ui-sql-editor")).toHaveAttribute(
      "data-line-wrapping",
      "true",
    );
    result.unmount();
  });

  it("configures disabled editors as non-editable", () => {
    codeMirrorViewMocks.editableOf.mockClear();
    const result = render(() => (
      <SqlQueryEditor
        value="SELECT 1"
        onChange={() => undefined}
        schema={buildSqlSchema([])}
        disabled
      />
    ));

    expect(codeMirrorViewMocks.editableOf).toHaveBeenCalledWith(false);
    expect(result.container.querySelector(".ui-sql-editor")).toHaveAttribute(
      "data-editor-editable",
      "false",
    );
    result.unmount();
  });

  it("propagates editor updates through the change listener", async () => {
    const [value, setValue] = createSignal(
      'SELECT * FROM "form_00000000000000000000000000000001"',
    );
    const onChange = vi.fn();

    const result = render(() => (
      <SqlQueryEditor
        value={value()}
        onChange={onChange}
        schema={buildSqlSchema([])}
      />
    ));

    setValue(
      'SELECT _ugoite_id FROM "form_00000000000000000000000000000001"',
    );

    await waitFor(() => {
      expect(onChange).toHaveBeenCalledWith(
        'SELECT _ugoite_id FROM "form_00000000000000000000000000000001"',
      );
    });

    result.unmount();
  });
});
