import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import type { JSX } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import CompositionNewRoute from "./new";

const { formsMock, savedSqlMock } = vi.hoisted(() => ({
  formsMock: vi.fn(),
  savedSqlMock: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  useLocation: () => ({ pathname: "/spaces/space-1/compositions/new" }),
  useParams: () => ({ space_id: "space-1" }),
  useSearchParams: () => [{}],
  useNavigate: () => vi.fn(),
  A: (props: { href: string; class?: string; children: JSX.Element }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
}));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: { list: (...args: unknown[]) => formsMock(...args) },
  sqlApi: { list: (...args: unknown[]) => savedSqlMock(...args) },
}));

describe("new Composition route", () => {
  beforeEach(() => {
    setLocale("en");
    formsMock.mockReset().mockResolvedValue([]);
    savedSqlMock.mockReset().mockResolvedValue([]);
  });

  it("opens the graph-only Design and Variables studio", async () => {
    render(() => <CompositionNewRoute />);

    expect(await screen.findByRole("tab", { name: "Design" }))
      .toBeVisible();
    expect(screen.getByRole("tab", { name: "Variables" })).toBeVisible();
    expect(screen.queryByRole("tab", { name: "Data" })).toBeNull();
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("");
    expect(screen.getByRole("button", { name: "Add to design" }))
      .toBeVisible();
  });
});
