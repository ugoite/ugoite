import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { For } from "solid-js";
import { setLocale } from "~/lib/i18n";
import SpaceQueryCreateRoute from "./new";

const {
  formApiListMock,
  navigateMock,
  parseSqlDocumentMock,
  sqlPlanMock,
} = vi.hoisted(() => ({
  formApiListMock: vi.fn(),
  navigateMock: vi.fn(),
  parseSqlDocumentMock: vi.fn(),
  sqlPlanMock: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; children: unknown }) => (
    <a href={props.href}>{props.children}</a>
  ),
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/components/SQLWorkspace", () => ({
  SQLWorkspaceActionBar: (props: {
    actions: readonly {
      label: string;
      disabled?: boolean;
      disabledReason?: string;
      onActivate: () => void;
    }[];
  }) => (
    <div role="group" aria-label="SQL">
      <For each={props.actions}>
        {(action) => (
          <button
            type="button"
            disabled={action.disabled}
            title={action.disabledReason}
            onClick={action.onActivate}
          >
            {action.label}
          </button>
        )}
      </For>
    </div>
  ),
  SQLWorkspace: () => <div data-testid="sql-workspace" />,
}));

vi.mock("~/lib/ugoite-client", () => ({
  parseSqlDocument: parseSqlDocumentMock,
  formApi: { list: formApiListMock },
  sqlApi: {
    create: vi.fn(),
    query: vi.fn(),
    count: vi.fn(),
    plan: sqlPlanMock,
  },
}));

describe("/spaces/:space_id/sql/new Form loading", () => {
  beforeEach(() => {
    setLocale("en");
    navigateMock.mockReset();
    formApiListMock.mockReset();
    parseSqlDocumentMock.mockReset().mockImplementation(async (
      source: string,
    ) => ({
      contract_version: 1,
      source,
      status: "parsed",
      diagnostics: [],
      tokens: [],
      nodes: [],
    }));
    sqlPlanMock.mockReset().mockResolvedValue({ result_schema: [] });
  });

  it("offers resolved Form names directly without the SQL relation tree", async () => {
    let resolveForms!: (forms: {
      id: string;
      name: string;
      sql_relation: string;
      version: number;
      template: string;
      fields: Record<string, never>;
    }[]) => void;
    formApiListMock.mockReturnValue(
      new Promise((resolve) => {
        resolveForms = resolve;
      }),
    );

    render(() => <SpaceQueryCreateRoute />);
    await waitFor(() =>
      expect(formApiListMock).toHaveBeenCalledWith("default")
    );

    expect(screen.getByRole("button", { name: "Form" })).toBeDisabled();

    resolveForms([{
      id: "orders-form",
      name: "Orders",
      sql_relation: "orders_relation",
      version: 1,
      template: "# Orders\n",
      fields: {},
    }]);

    parseSqlDocumentMock.mockResolvedValueOnce({
      contract_version: 1,
      source: "SELECT 1",
      status: "parsed",
      diagnostics: [],
      tokens: [],
      nodes: [
        {
          id: 0,
          parent: null,
          kind: "query_statement",
          range: null,
          semantic: null,
        },
        {
          id: 1,
          parent: 0,
          kind: "query",
          range: { start: 0, end: 8 },
          semantic: null,
        },
        {
          id: 2,
          parent: 1,
          kind: "select",
          range: { start: 0, end: 8 },
          semantic: null,
        },
        {
          id: 3,
          parent: 2,
          kind: "select_list",
          range: { start: 7, end: 8 },
          semantic: null,
        },
        {
          id: 4,
          parent: 3,
          kind: "select_item",
          range: { start: 7, end: 8 },
          semantic: null,
        },
        {
          id: 5,
          parent: 4,
          kind: "literal_expression",
          range: { start: 7, end: 8 },
          semantic: null,
        },
      ],
    });

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Form" })).toBeEnabled()
    );
    const formAction = screen.getByRole("button", { name: "Form" });
    fireEvent.click(formAction);

    const dialog = await screen.findByRole("dialog", { name: "Form" });
    expect(within(dialog).getByRole("button", { name: "Orders" }))
      .toBeInTheDocument();
    expect(within(dialog).queryByRole("tree")).not.toBeInTheDocument();
  });
});
