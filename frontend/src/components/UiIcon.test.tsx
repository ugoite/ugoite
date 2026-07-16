import "@testing-library/jest-dom/vitest";
import { render } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { UiIcon } from "./UiIcon";

describe("UiIcon", () => {
  it("renders every repeated icon instance instead of moving shared SVG nodes", () => {
    const { container } = render(() => (
      <div>
        <UiIcon name="plus" />
        <UiIcon name="plus" />
      </div>
    ));

    const icons = container.querySelectorAll("svg");
    expect(icons).toHaveLength(2);
    for (const icon of icons) {
      expect(icon.querySelectorAll("path")).toHaveLength(2);
      expect(icon).toHaveAttribute("stroke", "currentColor");
    }
  });
});
