import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CompositionParameterDefinition } from "~/lib/composition-api";
import { ParameterControl } from "./ParameterControl";

describe("Composition ParameterControl", () => {
  afterEach(() => cleanup());

  it("preserves timezone-bearing timestamp values in a text input", () => {
    const definition: CompositionParameterDefinition = {
      id: "created_at",
      type: "timestamp",
      required: true,
      default: "2026-10-13T14:59:59.123456789+09:00",
    };
    const onChange = vi.fn();
    render(() => (
      <ParameterControl
        definition={definition}
        displayName="Created at"
        value={undefined}
        onChange={onChange}
      />
    ));

    const input = screen.getByLabelText("Created at");
    expect(input).toHaveAttribute("type", "text");
    expect(input).toHaveValue("2026-10-13T14:59:59.123456789+09:00");

    fireEvent.input(input, {
      target: { value: "2026-10-13T05:59:59Z" },
    });
    expect(onChange).toHaveBeenCalledWith("2026-10-13T05:59:59Z");
  });
});
