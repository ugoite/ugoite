import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { BackLink } from "./BackLink";

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
}));

describe("BackLink", () => {
  beforeEach(() => {
    setLocale("en");
  });

  it("REQ-UX-NAV-001: renders the destination-named icon-only positional control", () => {
    render(() => (
      <BackLink
        href="/spaces/default/entries/entry-1"
        label="Back to Entry"
      />
    ));

    const back = screen.getByRole("link", { name: "Back to Entry" });
    expect(back).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-1",
    );
    expect(back).toHaveAttribute("title", "Back to Entry");
    expect(back).toHaveClass("pill", "iconpill", "icononly", "back-link");
    expect(back.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(back.querySelector(".ui-sr-only")).toHaveTextContent(
      "Back to Entry",
    );
    expect(back.childNodes).toHaveLength(2);
  });

  it("REQ-UX-NAV-001: keeps a localized accessible name for the icon-only control", () => {
    setLocale("ja");
    render(() => (
      <BackLink href="/spaces/default/sql" label="保存済み SQL に戻る" />
    ));

    const back = screen.getByRole("link", {
      name: "保存済み SQL に戻る",
    });
    expect(back.querySelector(".ui-sr-only")).toHaveTextContent(
      "保存済み SQL に戻る",
    );
    expect(back.querySelector("svg")).toBeInTheDocument();
  });

  it("REQ-UX-NAV-001: keeps a visible keyboard focus ring on the icon-only link", () => {
    render(() => (
      <BackLink
        href="/spaces/default/entries/entry-1"
        label="Back to Entry"
      />
    ));

    const back = screen.getByRole("link", { name: "Back to Entry" });
    expect(back).toHaveClass("back-link");
    expect(back).not.toHaveAttribute("tabindex", "-1");
  });
});
