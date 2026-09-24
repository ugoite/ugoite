import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import SpaceSqlDetailRoute from "./[sql_id]/index";
import { formatDateLabel } from "~/lib/date-format";
import {
  resetMockData,
  seedForm,
  seedSpace,
  seedSqlEntry,
} from "~/test/mocks/handlers";
import type { Space } from "~/lib/types";
import { sqlApi } from "~/lib/ugoite-client";

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
  SqlQueryEditor: (props: {
    id?: string;
    value: string;
    disabled?: boolean;
    onChange: (value: string) => void;
  }) => (
    <textarea
      id={props.id}
      data-testid="sql-editor"
      data-disabled={String(Boolean(props.disabled))}
      aria-label="SQL"
      value={props.value}
      disabled={props.disabled}
      onInput={(event) => props.onChange(event.currentTarget.value)}
    />
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
      kind: "user-query",
      sql:
        `SELECT * FROM "${entryRelation}" WHERE owner = {{owner}} ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
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

  it("REQ-FE-062: saved SQL detail renders an editable query workspace and supported actions", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Recent Search",
      kind: "user-query",
      sql:
        `SELECT * FROM "${entryRelation}" WHERE owner = {{owner}} ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
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
      "false",
    );
    expect(screen.getByRole("textbox", { name: "SQL" })).toHaveValue(
      `SELECT * FROM "${entryRelation}" WHERE owner = {{owner}} ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
    );
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
      kind: "user-query",
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

  it("normalizes SQL variables and uses the returned revision for the next save", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Planning",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const update = vi.spyOn(sqlApi, "update")
      .mockResolvedValueOnce({ id: "saved-query", revisionId: "rev-2" })
      .mockResolvedValueOnce({ id: "saved-query", revisionId: "rev-3" });

    render(() => <SpaceSqlDetailRoute />);
    const name = await screen.findByRole("textbox", { name: "Query name" });
    const editor = screen.getByRole("textbox", { name: "SQL" });
    fireEvent.input(name, { target: { value: "  Planning v2  " } });
    fireEvent.input(editor, {
      target: { value: "SELECT * FROM records WHERE title = {{title}}" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(update).toHaveBeenCalledTimes(1));
    expect(update).toHaveBeenNthCalledWith(1, "default", "saved-query", {
      name: "Planning v2",
      kind: "user-query",
      metadata: undefined,
      sql: "SELECT * FROM records WHERE title = $title",
      variables: [{ type: "string", name: "title", description: "" }],
      parent_revision_id: "rev-1",
    });

    fireEvent.input(editor, {
      target: { value: "SELECT * FROM records WHERE title = {{title}} LIMIT 2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(update).toHaveBeenCalledTimes(2));
    expect(update).toHaveBeenNthCalledWith(
      2,
      "default",
      "saved-query",
      expect.objectContaining({ parent_revision_id: "rev-2" }),
    );
    update.mockRestore();
  });

  it("saves a blank name with Untitled metadata and surfaces stale conflicts", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Planning",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const conflict = Object.assign(new Error("Revision conflict"), {
      status: 409,
      code: "REVISION_CONFLICT",
    });
    const update = vi.spyOn(sqlApi, "update").mockRejectedValue(conflict);

    render(() => <SpaceSqlDetailRoute />);
    const name = await screen.findByRole("textbox", { name: "Query name" });
    fireEvent.input(name, { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(update).toHaveBeenCalledWith(
      "default",
      "saved-query",
      expect.objectContaining({
        name: null,
        metadata: { generatedName: "untitled" },
        parent_revision_id: "rev-1",
      }),
    ));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(name).toHaveValue("   ");
    update.mockRestore();
  });

  it("deletes only after shared confirmation and returns to the Saved SQL list", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Planning",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const remove = vi.spyOn(sqlApi, "delete").mockResolvedValue(undefined);

    render(() => <SpaceSqlDetailRoute />);
    fireEvent.click(await screen.findByRole("button", { name: "Delete query" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete query" });
    expect(remove).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete query" }));

    await waitFor(() => expect(remove).toHaveBeenCalledWith("default", "saved-query"));
    expect(navigateMock).toHaveBeenCalledWith("/spaces/default/sql");
    remove.mockRestore();
  });

  it("keeps search-history detail read-only and retains its derived label", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: null,
      kind: "search-history",
      metadata: {
        searchCriteria: {
          formName: "Entry",
          tags: [],
          updatedFrom: "",
          updatedTo: "",
          fieldConditions: [],
        },
      },
      sql: "SELECT 1",
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });

    render(() => <SpaceSqlDetailRoute />);

    expect(await screen.findByRole("heading", { name: /Advanced search/ }))
      .toBeInTheDocument();
    expect(screen.getByTestId("sql-editor")).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Save" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete query" }))
      .not.toBeInTheDocument();
  });
});
