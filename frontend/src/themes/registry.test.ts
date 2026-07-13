import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

describe("v5 design system", () => {
  const css = fs.readFileSync(path.resolve(process.cwd(), "src/app.css"), "utf8");
  it("uses the approved concept palette", () => {
    expect(css).toContain("--bg: #f7f7f4");
    expect(css).toContain("--ink: #151515");
    expect(css).toContain("--line: #e4e1da");
    expect(css).toContain("--black: #111");
  });
  it("uses the approved shell dimensions and responsive breakpoint", () => {
    expect(css).toContain("grid-template-columns: 218px minmax(0, 1fr)");
    expect(css).toContain("height: 58px");
    expect(css).toContain("max-width: 1180px");
    expect(css).toContain("@media (max-width: 900px)");
    expect(css).toContain("height: 66px");
  });
  it("does not import the retired theme stylesheets", () => {
    expect(css).not.toContain("themes/classic.css");
    expect(css).not.toContain("themes/materialize.css");
    expect(css).not.toContain("themes/pop.css");
  });
});
