import "@testing-library/jest-dom/vitest";
import { fireEvent, render, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { buildSqlSchema } from "~/lib/sql";
import { SqlQueryEditor } from "./SqlQueryEditor";

const originalGetClientRects = Range.prototype.getClientRects;

beforeAll(() => {
  // jsdom does not implement Range geometry; CodeMirror only needs an empty
  // result to skip font measurements while these tests exercise history.
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
});

afterAll(() => {
  Range.prototype.getClientRects = originalGetClientRects;
});

describe("SqlQueryEditor DOM editability", () => {
  it("sets contenteditable false for read-only mode and updates with props", async () => {
    const [disabled, setDisabled] = createSignal(true);
    const result = render(() => (
      <SqlQueryEditor
        value="SELECT 1"
        onChange={() => undefined}
        schema={buildSqlSchema([])}
        disabled={disabled()}
      />
    ));

    const content = result.container.querySelector(".cm-content");
    expect(content).toHaveAttribute("contenteditable", "false");

    setDisabled(false);
    await waitFor(() => {
      expect(content).toHaveAttribute("contenteditable", "true");
    });

    setDisabled(true);
    await waitFor(() => {
      expect(content).toHaveAttribute("contenteditable", "false");
    });
    result.unmount();
  });

  it("undoes and redoes a SQL structure edit delivered through the value prop", async () => {
    const [value, setValue] = createSignal("SELECT 1");
    const result = render(() => (
      <SqlQueryEditor
        value={value()}
        onChange={setValue}
        schema={buildSqlSchema([])}
      />
    ));
    const content = result.container.querySelector(".cm-content");
    if (!content) throw new Error("CodeMirror content element is missing");

    // SQLWorkspace commits structural edits by updating this controlled value.
    setValue('SELECT "amount" FROM "G"');
    await waitFor(() => {
      expect(content).toHaveTextContent('SELECT "amount" FROM "G"');
    });

    content.focus();
    fireEvent.keyDown(content, { key: "z", ctrlKey: true });
    await waitFor(() => {
      expect(content).toHaveTextContent("SELECT 1");
      expect(value()).toBe("SELECT 1");
    });

    fireEvent.keyDown(content, { key: "y", ctrlKey: true });
    await waitFor(() => {
      expect(content).toHaveTextContent('SELECT "amount" FROM "G"');
      expect(value()).toBe('SELECT "amount" FROM "G"');
    });

    result.unmount();
  });
});
