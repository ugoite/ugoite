// REQ-FE-010: Counter component
import "@testing-library/jest-dom/vitest";
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import Counter from "./Counter";

describe("Counter", () => {
  it("renders initial count of 0", () => {
    render(() => <Counter />);
    expect(screen.getByRole("button")).toHaveTextContent("Clicks: 0");
  });

  it("increments count on click", () => {
    render(() => <Counter />);
    const btn = screen.getByRole("button");
    fireEvent.click(btn);
    expect(btn).toHaveTextContent("Clicks: 1");
    fireEvent.click(btn);
    expect(btn).toHaveTextContent("Clicks: 2");
  });
});
