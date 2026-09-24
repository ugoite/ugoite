// #2842 automated contract only: icon-only accessible names, disabled
// anchor no-activation, no bogus aria-label on generic spans, and the 44px
// target / 390px single-row CSS rules. Real-device iPhone Safari remains an
// optional manual checklist, never a gate.
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { ActionIconBar } from "./ActionIconBar";
import { GlobalShell } from "./GlobalShell";
import { IconButton } from "./IconButton";
import { IconLink } from "./IconLink";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; children: unknown }) => (
    <a href={props.href}>{props.children}</a>
  ),
}));

const stylesheet = () => readFileSync(join(__dirname, "..", "app.css"), "utf8");

describe("#2842 automated accessibility contract", () => {
  beforeEach(() => {
    setLocale("en");
  });

  it("gives every icon-only control an accessible name on the control itself", () => {
    const { container } = render(() => (
      <div>
        <IconButton icon="settings" label="Settings" />
        <IconLink icon="spaces" label="Spaces" href="/spaces" />
        <ActionIconBar
          label="Entry actions"
          actions={[
            { id: "save", icon: "save", label: "Save" },
            { id: "history", icon: "history", label: "History", href: "/h" },
          ]}
        />
      </div>
    ));

    expect(
      screen.getByRole("button", { name: "Settings" }).tagName,
    ).toBe("BUTTON");
    expect(screen.getByRole("link", { name: "Spaces" })).toHaveAttribute(
      "href",
      "/spaces",
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "History" })).toBeInTheDocument();
    // Decorative icon slots never introduce their own names: every svg is
    // hidden from assistive technology.
    for (const svg of container.querySelectorAll("svg")) {
      expect(svg).toHaveAttribute("aria-hidden", "true");
    }
  });

  it("never renders a disabled anchor: unavailable nav targets are omitted", () => {
    const { container } = render(() => (
      <ActionIconBar
        label="Entry actions"
        actions={[
          {
            id: "history",
            icon: "history",
            label: "History",
            href: "/history",
            disabled: true,
          },
        ]}
      />
    ));

    expect(container.querySelector("a")).toBeNull();
    expect(container.querySelector('[aria-disabled="true"]')).toBeNull();
  });

  it("keeps aria-label off generic spans", () => {
    const { container } = render(() => (
      <GlobalShell authenticated={false}>
        <p>content</p>
      </GlobalShell>
    ));

    // The assistive-technology indicator keeps its sr-only text; the generic
    // span itself carries no (bogus) aria-label.
    expect(container.querySelector("span[aria-label]")).toBeNull();
    expect(screen.getByText("Konase")).toBeInTheDocument();
  });

  it("declares 44px minimum targets for every action-bar and icon-only rule", () => {
    const css = stylesheet();
    const block = (selector: string): string => {
      const start = css.indexOf(selector);
      expect(start, `${selector} rule`).toBeGreaterThan(-1);
      const open = css.indexOf("{", start);
      const close = css.indexOf("}", open);
      return css.slice(open, close);
    };
    for (
      const selector of [
        ".tool {",
        ".pill.iconpill {",
        ".icononly {",
        ".ui-entry-tool {",
      ]
    ) {
      const declarations = block(selector);
      expect(declarations, `${selector} min-width`).toMatch(
        /min-width:\s*44px/,
      );
      expect(declarations, `${selector} min-height`).toMatch(
        /min-height:\s*44px/,
      );
    }
  });

  it("keeps the one-row action bar at 390px widths", () => {
    const css = stylesheet();
    // The narrow-viewport override keeps four flexible columns with no
    // wrapping or horizontal scrolling.
    expect(css).toMatch(
      /@media\s*\(max-width:\s*560px\)[\s\S]*?\.actionbar\.compact-actions\s*\{[\s\S]*?grid-template-columns:\s*repeat\(4,\s*minmax\(0,\s*1fr\)\)/,
    );
  });
});
