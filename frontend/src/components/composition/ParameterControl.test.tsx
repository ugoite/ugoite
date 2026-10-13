import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CompositionVariableDefinition } from "~/lib/composition-api";
import { ParameterControl } from "./ParameterControl";

describe("Composition ParameterControl", () => {
  afterEach(() => cleanup());

  it("preserves timezone-bearing timestamp values in a text input", async () => {
    const definition: CompositionVariableDefinition = {
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
    await waitFor(() => {
      expect(onChange).toHaveBeenCalledWith("2026-10-13T05:59:59Z");
    });
  });

  it("uses the Domain parser for numeric Inputs without coercing invalid text", async () => {
    const definition: CompositionVariableDefinition = {
      id: "limit",
      type: "integer",
      required: true,
      default: 0,
    };
    const onChange = vi.fn();
    render(() => (
      <ParameterControl
        definition={definition}
        displayName="Limit"
        value={undefined}
        onChange={onChange}
      />
    ));

    const input = screen.getByLabelText("Limit");
    expect(input).toHaveAttribute("type", "text");

    fireEvent.input(input, { target: { value: "+00042" } });
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(42));

    fireEvent.input(input, { target: { value: "1.0" } });
    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith("1.0"));
    expect(input).toHaveValue("1.0");
    expect(input).toHaveAttribute("aria-invalid", "true");
  });

  it("keeps an underflowing Float token raw instead of converting it to zero", async () => {
    const definition: CompositionVariableDefinition = {
      id: "ratio",
      type: "float",
      required: false,
    };
    const onChange = vi.fn();
    render(() => (
      <ParameterControl
        definition={definition}
        displayName="Ratio"
        value={undefined}
        onChange={onChange}
      />
    ));

    const input = screen.getByLabelText("Ratio");
    fireEvent.input(input, { target: { value: "1e-4000" } });
    await waitFor(() => {
      expect(onChange).toHaveBeenLastCalledWith("1e-4000");
    });
    expect(input).toHaveValue("1e-4000");
    expect(input).toHaveAttribute("aria-invalid", "true");
  });
});
