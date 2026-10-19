import "@testing-library/jest-dom/vitest";
import { render, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { describe, expect, it } from "vitest";
import { buildSqlSchema } from "~/lib/sql";
import { SqlQueryEditor } from "./SqlQueryEditor";

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
});
