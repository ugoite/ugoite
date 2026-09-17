import "@testing-library/jest-dom/vitest";
import { render } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { UiIcon, type UiIconName } from "./UiIcon";

const allNames: UiIconName[] = [
  "home",
  "forms",
  "search",
  "settings",
  "spaces",
  "menu",
  "plus",
  "entry",
  "asset",
  "sql",
  "members",
  "agent",
  "credential",
  "storage",
  "appearance",
  "history",
  "refresh",
  "info",
  "trash",
  "preview",
  "download",
  "close",
];

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

  it("honours the single 24x24 contract with unified stroke", () => {
    for (const name of allNames) {
      const { container, unmount } = render(() => <UiIcon name={name} />);
      const svg = container.querySelector("svg");
      expect(svg).toHaveAttribute("viewBox", "0 0 24 24");
      expect(svg).toHaveAttribute("stroke-width", "1.9");
      expect(svg).toHaveAttribute("vector-effect", "non-scaling-stroke");
      expect(svg?.getAttribute("class")).toContain("icon");
      unmount();
    }
  });

  it("gives forms, spaces, menu, and appearance distinct glyphs", () => {
    const markup = (name: UiIconName) => {
      const { container, unmount } = render(() => <UiIcon name={name} />);
      const html = container.innerHTML;
      unmount();
      return html;
    };
    const forms = markup("forms");
    const spaces = markup("spaces");
    const menu = markup("menu");
    const appearance = markup("appearance");
    const glyphs = new Set([forms, spaces, menu, appearance]);
    expect(glyphs.size).toBe(4);
    // forms is a document/list tile, spaces is stacked layers.
    expect(forms).toContain("<rect");
    expect(spaces).toContain("M4 7.5");
  });
});
