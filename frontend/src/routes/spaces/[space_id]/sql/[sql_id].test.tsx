import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import SpaceSqlDetailRoute from "./[sql_id]/index";
import { formatDateLabel } from "~/lib/date-format";
import {
  resetMockData,
  seedForm,
  seedSpace,
  seedSqlEntry,
} from "~/test/mocks/handlers";
import type { Space } from "~/lib/types";

const navigateMock = vi.fn();
const entryRelation = "form_00000000000000000000000000000001";

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
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default", sql_id: "saved-query" }),
}));

vi.mock("~/components", () => ({
  SqlQueryEditor: (props: { value: string; disabled?: boolean }) => (
    <pre
      data-testid="sql-editor"
      data-disabled={String(Boolean(props.disabled))}
    >
			{props.value}
    </pre>
  ),
}));

describe("/spaces/:space_id/sql/:sql_id", () => {
  const testSpace: Space = {
    space_uid: "default",
    name: "Default",
    created_at: "2025-01-01T00:00:00Z",
  };

  beforeEach(() => {
    navigateMock.mockReset();
    resetMockData();
    seedSpace(testSpace);
  });

  it("REQ-UX-NAV-001: exposes exactly one back control to saved SQL", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Recent Search",
      sql:
        `SELECT * FROM "${entryRelation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });

    render(() => <SpaceSqlDetailRoute />);

    const backLink = await screen.findByRole("link", {
      name: "Back to Saved SQL",
    });
    expect(backLink).toHaveAttribute("href", "/spaces/default/sql");
    expect(screen.getAllByRole("link", { name: "Back to Saved SQL" }))
      .toHaveLength(1);
    // Shell destinations stay in the shell: no route-level shortcuts.
    expect(screen.queryByRole("link", { name: "Back to Dashboard" }))
      .not.toBeInTheDocument();
  });

  it("REQ-FE-062: saved SQL detail renders a read-only query summary and supported actions", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Recent Search",
      sql:
        `SELECT * FROM "${entryRelation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
      variables: [{
        name: "owner",
        type: "string",
        description: "Owner filter",
      }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });

    render(() => <SpaceSqlDetailRoute />);

    expect(await screen.findByRole("heading", { name: "Recent Search" }))
      .toBeInTheDocument();
    expect(screen.getByText(formatDateLabel("2025-03-02T00:00:00Z")))
      .toBeInTheDocument();
    expect(screen.getByText(formatDateLabel("2025-03-01T00:00:00Z")))
      .toBeInTheDocument();
    expect(screen.getByTestId("sql-editor")).toHaveAttribute(
      "data-disabled",
      "true",
    );
    expect(screen.getByText(
      `SELECT * FROM "${entryRelation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
    ))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open Variables" }))
      .toHaveAttribute(
        "href",
        "/spaces/default/sql/saved-query/variables",
      );
    expect(screen.getByRole("link", { name: "Back to Saved SQL" }))
      .toHaveAttribute(
        "href",
        "/spaces/default/sql",
      );
    // Shell destinations stay in the shell: no route-level shortcuts.
    expect(screen.queryByRole("link", { name: "Open Search" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Back to Dashboard" }))
      .not.toBeInTheDocument();
  });

  it("REQ-FE-062: saved SQL detail routes variable-free queries to stateless results", async () => {
    seedForm("default", {
      name: "Entry",
      sql_relation: entryRelation,
      version: 1,
      template: "# Entry\n",
      fields: {},
    });
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Runnable Query",
      sql:
        `SELECT * FROM "${entryRelation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 1`,
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    render(() => <SpaceSqlDetailRoute />);
    fireEvent.click(await screen.findByRole("button", { name: "Run Query" }));

    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/default/sql/saved-query/run",
    );
  });
});
