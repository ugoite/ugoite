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

  it("REQ-UX-NAV-001: renders a positional back label with the destination as accessible name and tooltip", () => {
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
    expect(back).toHaveTextContent("Back");
    expect(back.textContent).not.toMatch(/Back to Entry/);
  });

  it("REQ-UX-NAV-001: uses the Japanese short label", () => {
    setLocale("ja");
    render(() => (
      <BackLink href="/spaces/default/sql" label="保存済み SQL に戻る" />
    ));

    const back = screen.getByRole("link", {
      name: "保存済み SQL に戻る",
    });
    expect(back).toHaveTextContent("戻る");
  });
});
