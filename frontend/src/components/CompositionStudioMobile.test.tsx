import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CompositionStudio } from "./CompositionStudio";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { setLocale } from "~/lib/i18n";

const { navigateMock, formsMock, savedSqlMock, routePathMock } = vi.hoisted(
  () => ({
    navigateMock: vi.fn(),
    formsMock: vi.fn(),
    savedSqlMock: vi.fn(),
    routePathMock: { value: "/spaces/space-1/compositions/new" },
  }),
);

vi.mock("@solidjs/router", () => ({
  useLocation: () => ({
    get pathname() {
      return routePathMock.value;
    },
  }),
  useNavigate: () => navigateMock,
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children as never}</a>
  ),
}));

vi.mock("~/lib/ugoite-client", async (importOriginal) => ({
  compositionFormQuerySql:
    (await importOriginal<typeof import("~/lib/ugoite-client")>())
      .compositionFormQuerySql,
  formApi: { list: (...args: unknown[]) => formsMock(...args) },
  sqlApi: { list: (...args: unknown[]) => savedSqlMock(...args) },
}));

const tableDraft = (): CompositionGraphDraft => {
  const draft = createCompositionGraphDraft("Mobile");
  draft.variables.push({
    id: "rows",
    kind: "query",
    returns: { kind: "table", columns: [] },
    query: {
      kind: "inline_sql",
      sql: 'SELECT * FROM "form_1"',
      arguments: [],
      form_bindings: [],
      schema_fingerprint: "",
    },
  });
  return draft;
};

describe("CompositionStudio narrow binding picker", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    routePathMock.value = "/spaces/space-1/compositions/new";
    formsMock.mockResolvedValue([]);
    savedSqlMock.mockResolvedValue([]);
  });
  afterEach(() => cleanup());

  it("keeps one modal owner and restores focus after Escape", async () => {
    const appRoot = document.createElement("div");
    appRoot.id = "app";
    document.body.append(appRoot);
    render(() => (
      <CompositionStudio
        spaceId="space-1"
        initialDraft={tableDraft()}
        saveMode={{ kind: "create" }}
        backHref="/spaces/space-1/compositions"
        backLabel="Back to compositions"
      />
    ));

    const add = screen.getByRole("button", { name: "Add to design" });
    add.focus();
    fireEvent.click(add);
    const table = screen.getByRole("button", { name: "Table" });
    table.focus();
    fireEvent.click(table);

    const picker = screen.getByRole("dialog", { name: "Table" });
    expect(screen.getAllByRole("dialog")).toEqual([picker]);
    expect(picker).toHaveAttribute("aria-modal", "true");
    expect(appRoot).toHaveAttribute("inert");
    expect(within(picker).getByRole("tab", { name: "Variables" }))
      .toHaveAttribute(
        "aria-selected",
        "true",
      );
    expect(within(picker).getByRole("tab", { name: "Forms" }))
      .toBeInTheDocument();
    expect(within(picker).getByRole("tab", { name: "Saved SQL" }))
      .toBeInTheDocument();

    fireEvent.keyDown(picker, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(appRoot).not.toHaveAttribute("inert");
      expect(add).toHaveFocus();
    });
    appRoot.remove();
  });
});
