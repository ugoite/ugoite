import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CompositionVariableDefinition } from "~/lib/composition-api";
import {
  parseCompositionVariableInputSync,
  prepareCompositionVariableInputParser,
} from "~/lib/ugoite-client/protocol";
import { InputControl } from "./InputControl";

describe("Composition InputControl", () => {
  afterEach(() => cleanup());

  it("preserves timezone-bearing timestamps in the shared raw input state", () => {
    const definition: CompositionVariableDefinition = {
      id: "created_at",
      type: "timestamp",
      required: true,
      default: "2026-10-13T14:59:59.123456789+09:00",
    };
    const [raw, setRaw] = createSignal(definition.default! as string);
    const onChange = vi.fn((value: string) => setRaw(value));
    render(() => (
      <InputControl
        definition={definition}
        displayName="Created at"
        value={undefined}
        rawValue={raw()}
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
    expect(input).toHaveValue("2026-10-13T05:59:59Z");
  });

  it("synchronously shares valid typed values and invalid raw text across Inputs", async () => {
    await prepareCompositionVariableInputParser();
    const definition: CompositionVariableDefinition = {
      id: "limit",
      type: "integer",
      required: true,
      default: 0,
    };
    const [raw, setRaw] = createSignal("0");
    const [value, setValue] = createSignal<unknown>(0);
    const [invalid, setInvalid] = createSignal(false);
    const onInput = (next: string) => {
      setRaw(next);
      const parsed = parseCompositionVariableInputSync("integer", next);
      if (parsed.status === "valid") {
        setValue(parsed.value);
        setInvalid(false);
      } else if (parsed.status === "unset") {
        setValue(null);
        setInvalid(false);
      } else {
        setValue(null);
        setInvalid(true);
      }
    };
    const { container } = render(() => (
      <>
        <InputControl
          definition={definition}
          displayName="First limit"
          value={value()}
          rawValue={raw()}
          invalid={invalid()}
          onChange={onInput}
        />
        <InputControl
          definition={definition}
          displayName="Second limit"
          value={value()}
          rawValue={raw()}
          invalid={invalid()}
          onChange={onInput}
        />
      </>
    ));

    const first = screen.getByLabelText("First limit");
    const second = screen.getByLabelText("Second limit");
    fireEvent.input(first, { target: { value: "+00042" } });
    expect(first).toHaveValue("+00042");
    expect(second).toHaveValue("+00042");
    expect(value()).toBe(42);
    expect(first).not.toHaveAttribute("aria-invalid");

    fireEvent.input(second, { target: { value: "1.0" } });
    expect(container.querySelectorAll('input[aria-invalid="true"]'))
      .toHaveLength(2);
    expect(first).toHaveValue("1.0");
    expect(second).toHaveValue("1.0");
    expect(value()).toBeNull();
  });
});
