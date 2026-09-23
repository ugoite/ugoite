import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formApi } from "~/lib/ugoite-client";
import SpaceFormTypesRoute from "./types";

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
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: { listTypes: vi.fn() },
}));

describe("form types route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
    vi.mocked(formApi.listTypes).mockResolvedValue(["string", "double"]);
  });

  it("REQ-UX-NAV-001: exposes exactly one back control to forms", async () => {
    render(() => <SpaceFormTypesRoute />);

    expect(await screen.findByText("string")).toBeInTheDocument();
    const backLink = screen.getByRole("link", { name: "Back to Forms" });
    expect(backLink).toHaveAttribute("href", "/spaces/default/forms");
    expect(backLink).toHaveAttribute("title", "Back to Forms");
    expect(screen.getAllByRole("link", { name: "Back to Forms" }))
      .toHaveLength(1);
  });

  it("REQ-UX-NAV-001: states the form context once without a competing header", async () => {
    render(() => <SpaceFormTypesRoute />);

    expect(await screen.findByText("string")).toBeInTheDocument();
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Form Field Types" }))
      .toBeInTheDocument();
  });
});
