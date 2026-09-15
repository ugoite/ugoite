import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

describe("v5 design system", () => {
  const css = fs.readFileSync(
    path.resolve(process.cwd(), "src/app.css"),
    "utf8",
  );
  it("uses the approved concept palette", () => {
    expect(css).toContain("--bg: #F6F8FC");
    expect(css).toContain("--ink: #10243C");
    expect(css).toContain("--faint: #5D6F86");
    expect(css).toContain("--line: #D7E0EA");
    expect(css).toContain("--black: #111");
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
