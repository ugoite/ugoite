import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { FieldStack, FieldStackRow } from "./FieldStack";

describe("FieldStack", () => {
  it("REQ-UX-ENTRY-001: renders field editors as plain list rows without card chrome", () => {
    const { container } = render(() => (
      <FieldStack label="Columns">
        <FieldStackRow>
          <label>
            Title
            <input type="text" value="a" readOnly />
          </label>
        </FieldStackRow>
        <FieldStackRow>
          <label>
            Count
            <input type="text" value="1" readOnly />
          </label>
        </FieldStackRow>
      </FieldStack>
    ));

    const list = screen.getByRole("list", { name: "Columns" });
    expect(list).toHaveClass("fieldStack");
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
    // Plain rows: no card containers around editors or guidance.
    expect(container.querySelector(".fieldStack .ui-card")).toBeNull();
    expect(screen.getByLabelText("Title")).toBeInTheDocument();
    expect(screen.getByLabelText("Count")).toBeInTheDocument();
  });

  it("REQ-UX-RESP-001: keeps rows in a single stacked column at any viewport", () => {
    const { container } = render(() => (
      <FieldStack label="Columns">
        <FieldStackRow>
          <input type="text" aria-label="first field" value="x" readOnly />
        </FieldStackRow>
        <FieldStackRow>
          <input type="text" aria-label="second field" value="y" readOnly />
        </FieldStackRow>
      </FieldStack>
    ));

    // One stacked column: the list itself carries the single-column stack
    // hook, and every row is a direct child (no wrapping intermediaries
    // that could force a second column or horizontal overflow at 390px).
    const list = container.querySelector(".fieldStack")!;
    expect(list).toHaveClass("ui-stack-sm");
    expect(Array.from(list.children)).toHaveLength(2);
    for (const row of Array.from(list.children)) {
      expect(row).toHaveClass("fieldStackRow");
      expect(row).toHaveAttribute("role", "listitem");
    }
  });
});
