import "@testing-library/jest-dom/vitest";
import { render } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { ButtonSpinner } from "./ButtonSpinner";

describe("ButtonSpinner", () => {
  it("renders a small local signal with no visible text", () => {
    const { container } = render(() => <ButtonSpinner />);

    const spinner = container.querySelector(".btnSpinner");
    expect(spinner).toBeInTheDocument();
    expect(spinner).toHaveAttribute("aria-hidden", "true");
    expect(spinner?.textContent).toBe("");
  });
});
