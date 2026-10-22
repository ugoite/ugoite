import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

describe("v5 design system", () => {
  const css = fs.readFileSync(
    path.resolve(process.cwd(), "src/app.css"),
    "utf8",
  );
  it("uses the approved concept palette", () => {
    expect(css.toLowerCase()).toContain("--bg: #f6f8fc");
    expect(css.toLowerCase()).toContain("--ink: #10243c");
    expect(css.toLowerCase()).toContain("--faint: #5d6f86");
    expect(css.toLowerCase()).toContain("--line: #d7e0ea");
    expect(css.toLowerCase()).toContain("--black: #111");
  });
  it("uses the approved shell dimensions and responsive breakpoint", () => {
    expect(css).toContain("grid-template-columns: 228px minmax(0, 1fr)");
    expect(css).toContain("height: 58px");
    expect(css).toContain("max-width: 1230px");
    expect(css).toContain("@media (max-width: 900px)");
    expect(css).toContain("height: 66px");
  });
  it("does not import the retired theme stylesheets", () => {
    expect(css).not.toContain("themes/classic.css");
    expect(css).not.toContain("themes/materialize.css");
    expect(css).not.toContain("themes/pop.css");
  });
  it("keeps dark-mode surfaces readable without theme or accent variants", () => {
    expect(css).toContain(':root[data-color-mode="dark"]');
    expect(css).toContain("--panel: #1b1d22");
    expect(css).toContain("--muted: #b8bbc4");
    expect(css).toContain("--faint: #8f98a8");
    expect(css).not.toContain("data-ui-theme");
    expect(css).not.toContain("data-primary-color");
  });
});
