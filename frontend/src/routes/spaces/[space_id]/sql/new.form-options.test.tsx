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
  analyzeSqlStructureMock,
  formApiListMock,
  navigateMock,
  parseSqlDocumentMock,
  prepareSqlStructureParserMock,
  proposeSqlStructureOperationMock,
} = vi.hoisted(() => ({
  analyzeSqlStructureMock: vi.fn(),
  formApiListMock: vi.fn(),
  navigateMock: vi.fn(),
  parseSqlDocumentMock: vi.fn(),
  prepareSqlStructureParserMock: vi.fn(),
  proposeSqlStructureOperationMock: vi.fn(),
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
  analyzeSqlStructure: analyzeSqlStructureMock,
  applySqlDocumentEditTransaction: vi.fn(),
  applySqlDocumentNodeEditTransaction: vi.fn(),
  applySqlDocumentReplacementTransaction: vi.fn(),
  parseSqlDocument: parseSqlDocumentMock,
  prepareSqlStructureParser: prepareSqlStructureParserMock,
  proposeSqlStructureOperation: proposeSqlStructureOperationMock,
  formApi: { list: formApiListMock },
  sqlApi: {
    create: vi.fn(),
    query: vi.fn(),
    count: vi.fn(),
  },
}));

describe("/spaces/:space_id/sql/new Form loading", () => {
  beforeEach(() => {
    setLocale("en");
    navigateMock.mockReset();
    formApiListMock.mockReset();
    parseSqlDocumentMock.mockReset().mockImplementation(async (source: string) => ({
      contract_version: 1,
      source,
      status: "parsed",
      diagnostics: [],
      tokens: [],
      nodes: [],
    }));
    prepareSqlStructureParserMock.mockReset().mockResolvedValue(undefined);
    analyzeSqlStructureMock.mockReset().mockImplementation(
      (_sql: string, context: { relation: string }) =>
        Promise.resolve({
          contract_version: 1,
          status: context.relation ? "supported" : "unsupported",
          projection: ["*"],
          filters: [],
          aggregates: [],
          group_by: [],
          distinct: false,
          order_by: [],
          referenced_variables: [],
          stable_pagination: false,
          capabilities: {
            columns: { enabled: false },
            filter: { enabled: false },
            sort: { enabled: false },
            aggregate: { enabled: false },
            group_by: { enabled: false },
            distinct: { enabled: false },
            stable_pagination: { enabled: false },
          },
        }),
    );
    proposeSqlStructureOperationMock.mockReset();
  });

  it("enables the real Form selector after the asynchronous Form list resolves", async () => {
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

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Form" })).toBeEnabled()
    );
    const formAction = screen.getByRole("button", { name: "Form" });
    fireEvent.click(formAction);

    const dialog = await screen.findByRole("dialog", { name: "Form" });
    expect(
      within(dialog).getByRole("button", { name: "Orders" }),
    ).toBeEnabled();
  });
});
