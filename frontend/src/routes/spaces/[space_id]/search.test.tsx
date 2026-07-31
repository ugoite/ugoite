import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { http, HttpResponse } from "msw";
import SpaceSearchRoute from "./search";
import {
  resetMockData,
  seedEntry,
  seedForm,
  seedSpace,
  seedSqlEntry,
} from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Entry, EntryRecord, Form, Space } from "~/lib/types";
import { testApiUrl } from "~/test/http-origin";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/components/SpaceShell", () => ({
  SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));

describe("/spaces/:space_id/search", () => {
  const testSpace: Space = {
    id: "default",
    name: "Default",
    created_at: "2025-01-01T00:00:00Z",
  };

  beforeEach(() => {
    navigateMock.mockReset();
    resetMockData();
    seedSpace(testSpace);
  });

  it("REQ-FE-054: renders human-readable updated dates in search history", async () => {
    seedSqlEntry("default", {
      id: "query-1",
      name: "Recent Search",
      sql: "SELECT * FROM entries LIMIT 10",
      variables: [],
      created_at: 1772960822.056,
      updated_at: 1772960822.056,
      revision_id: "rev-1",
    });

    render(() => <SpaceSearchRoute />);

    const expectedDate = new Date(1772960822.056 * 1000).toLocaleDateString();
    expect(await screen.findByRole("button", { name: /Recent Search/ }))
      .toBeInTheDocument();
    expect(await screen.findByText(`Updated ${expectedDate}`))
      .toBeInTheDocument();
    expect(screen.queryByText("Updated 1772960822.056")).not
      .toBeInTheDocument();
  });

  it("REQ-SRCH-004: runs a direct keyword search and renders matching entries", async () => {
    const entry: Entry = {
      id: "entry-1",
      title: "Alpha Entry",
      content:
        "---\nform: Entry\n---\n# Alpha Entry\n\n## Body\nKeyword-first search is easier.",
      revision_id: "rev-1",
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-02T00:00:00Z",
    };
    const record: EntryRecord = {
      id: entry.id,
      title: entry.title ?? "Alpha Entry",
      form: "Entry",
      updated_at: entry.updated_at,
      properties: { Body: "Keyword-first search is easier." },
      tags: ["search"],
      links: [],
    };
    seedEntry("default", entry, record);
    let entryListCalls = 0;
    let sqlSessionCalls = 0;
    server.use(
      http.get(testApiUrl("/spaces/default/entries"), () => {
        entryListCalls += 1;
        return HttpResponse.json([]);
      }),
      http.get(
        testApiUrl("/spaces/default/search"),
        () => HttpResponse.json([record]),
      ),
      http.post(testApiUrl("/spaces/default/sql-sessions"), () => {
        sqlSessionCalls += 1;
        return HttpResponse.json(
          { detail: "Quick search must not create a SQL session" },
          { status: 500 },
        );
      }),
    );

    render(() => <SpaceSearchRoute />);

    fireEvent.input(screen.getByLabelText("Search keywords"), {
      target: { value: "keyword-first" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));

    expect(await screen.findByRole("button", { name: /Alpha Entry/ }))
      .toBeInTheDocument();
    expect(screen.getByText("1 result")).toBeInTheDocument();
    expect(entryListCalls).toBe(0);
    expect(sqlSessionCalls).toBe(0);
  });

  it("REQ-SRCH-005: advanced search compiles filters into saved SQL and runs a shared session", async () => {
    const meetingForm: Form = {
      name: "Meeting",
      sql_relation: "meeting",
      version: 1,
      template: "# Meeting\n\n## Status\n",
      fields: {
        Status: { type: "string", required: false, sql_column: "field_100" },
      },
    };
    seedForm("default", meetingForm);

    let savedSqlBody: { name?: string; sql?: string } | null = null;
    let sessionSqlBody: {
      sql?: string;
      parameters?: Record<string, unknown>;
      parameter_types?: Record<string, string>;
    } | null = null;
    const postOrder: string[] = [];

    server.use(
      http.post(testApiUrl("/spaces/default/sql"), async ({ request }) => {
        postOrder.push("saved");
        savedSqlBody = (await request.json()) as {
          name?: string;
          sql?: string;
        };
        return HttpResponse.json(
          { id: "saved-search-1", revision_id: "rev-2" },
          { status: 201 },
        );
      }),
      http.post(
        testApiUrl("/spaces/default/sql-sessions"),
        async ({ request }) => {
          postOrder.push("session");
          sessionSqlBody = (await request.json()) as typeof sessionSqlBody;
          return HttpResponse.json(
            { id: "advanced-session", status: "ready", error: null },
            { status: 201 },
          );
        },
      ),
    );

    render(() => <SpaceSearchRoute />);

    fireEvent.click(screen.getByRole("button", { name: "Advanced search" }));
    await screen.findByRole("option", { name: "Meeting" });
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "Meeting" },
    });
    fireEvent.input(screen.getByLabelText("Updated from"), {
      target: { value: "2025-03-01" },
    });
    fireEvent.input(screen.getByLabelText("Updated to"), {
      target: { value: "2025-03-03" },
    });
    await screen.findByRole("option", { name: "Status" });
    fireEvent.change(screen.getByLabelText("Field"), {
      target: { value: "Status" },
    });
    fireEvent.input(screen.getByLabelText("Value"), {
      target: { value: "Active" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Run advanced search" }),
    );

    await waitFor(() => {
      expect(savedSqlBody?.name).toBe(
        "Advanced search - form: Meeting - updated-from: 2025-03-01 - updated-to: 2025-03-03 - Status=Active",
      );
      expect(savedSqlBody?.sql).toBe(
        "SELECT * FROM \"meeting\" WHERE _ugoite_updated_at >= TIMESTAMP '2025-03-01 00:00:00Z' AND _ugoite_updated_at < TIMESTAMP '2025-03-04 00:00:00Z' AND \"field_100\" = 'Active' ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50",
      );
      expect(sessionSqlBody?.sql).toBe(
        'SELECT * FROM "meeting" WHERE _ugoite_updated_at >= $search_0 AND _ugoite_updated_at < $search_1 AND "field_100" = $search_2 ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50',
      );
      expect(sessionSqlBody?.parameters).toEqual({
        search_0: "2025-03-01T00:00:00.000Z",
        search_1: "2025-03-04T00:00:00.000Z",
        search_2: "Active",
      });
      expect(sessionSqlBody?.parameter_types).toEqual({
        search_0: "timestamp",
        search_1: "timestamp",
        search_2: "string",
      });
      expect(postOrder).toEqual(["session", "saved"]);
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/entries?session=advanced-session",
      );
    });
  });

  it("uses native typed parameters for numeric and boolean fields", async () => {
    seedForm("default", {
      name: "Metrics",
      sql_relation: "metrics",
      version: 1,
      template: "",
      fields: {
        Count: { type: "integer", required: false, sql_column: "field_100" },
        Enabled: { type: "boolean", required: false, sql_column: "field_101" },
      },
    });
    let sessionBody: {
      sql?: string;
      parameters?: Record<string, unknown>;
      parameter_types?: Record<string, string>;
    } | null = null;
    server.use(
      http.post(
        testApiUrl("/spaces/default/sql-sessions"),
        async ({ request }) => {
          sessionBody = (await request.json()) as typeof sessionBody;
          return HttpResponse.json({
            id: "typed-session",
            status: "ready",
            error: null,
          }, { status: 201 });
        },
      ),
      http.post(
        testApiUrl("/spaces/default/sql"),
        () =>
          HttpResponse.json({ id: "typed-saved", revision_id: "rev-typed" }, {
            status: 201,
          }),
      ),
    );
    render(() => <SpaceSearchRoute />);
    fireEvent.click(screen.getByRole("button", { name: "Advanced search" }));
    await screen.findByRole("option", { name: "Metrics" });
    fireEvent.change(await screen.findByLabelText("Form"), {
      target: { value: "Metrics" },
    });
    const fields = await screen.findAllByLabelText("Field");
    fireEvent.change(fields[0], { target: { value: "Count" } });
    fireEvent.input(screen.getAllByLabelText("Value")[0], {
      target: { value: "10" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Add field condition" }),
    );
    const booleanFields = await screen.findAllByLabelText("Field");
    fireEvent.change(booleanFields[1], { target: { value: "Enabled" } });
    fireEvent.input(screen.getAllByLabelText("Value")[1], {
      target: { value: "true" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Run advanced search" }),
    );
    await waitFor(() => {
      expect(sessionBody?.parameters).toEqual({ search_0: 10, search_1: true });
      expect(sessionBody?.parameter_types).toEqual({
        search_0: "integer",
        search_1: "boolean",
      });
      expect(sessionBody?.sql).toContain('"field_100" = $search_0');
      expect(sessionBody?.sql).toContain('"field_101" = $search_1');
    });
  });

  it("does not offer long or nanosecond fields as approximate Advanced search types", async () => {
    seedForm("default", {
      name: "Precise metrics",
      sql_relation: "form_precise_metrics",
      version: 1,
      template: "",
      fields: {
        LargeCount: { type: "long", required: false, sql_column: "field_100" },
        PreciseTime: {
          type: "timestamp_ns",
          required: false,
          sql_column: "field_101",
        },
      },
    });
    render(() => <SpaceSearchRoute />);
    fireEvent.click(screen.getByRole("button", { name: "Advanced search" }));
    await screen.findByRole("option", { name: "Precise metrics" });
    fireEvent.change(await screen.findByLabelText("Form"), {
      target: { value: "Precise metrics" },
    });

    expect(
      await screen.findByRole("option", { name: "LargeCount (unsupported)" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("option", { name: "PreciseTime (unsupported)" }),
    ).toBeDisabled();
  });

  it("resets field conditions when the selected Form changes", async () => {
    seedForm("default", {
      name: "Form A",
      sql_relation: "form_a",
      version: 1,
      template: "",
      fields: {
        Status: { type: "string", required: false, sql_column: "field_100" },
      },
    });
    seedForm("default", {
      name: "Form B",
      sql_relation: "form_b",
      version: 1,
      template: "",
      fields: {
        Owner: { type: "string", required: false, sql_column: "field_101" },
      },
    });
    let sessionSql = "";
    server.use(
      http.post(
        testApiUrl("/spaces/default/sql-sessions"),
        async ({ request }) => {
          sessionSql = ((await request.json()) as { sql: string }).sql;
          return HttpResponse.json({
            id: "form-switch-session",
            status: "ready",
            error: null,
          }, { status: 201 });
        },
      ),
      http.post(
        testApiUrl("/spaces/default/sql"),
        () =>
          HttpResponse.json({
            id: "form-switch-saved",
            revision_id: "rev-switch",
          }, { status: 201 }),
      ),
    );
    render(() => <SpaceSearchRoute />);
    fireEvent.click(screen.getByRole("button", { name: "Advanced search" }));
    const formSelect = await screen.findByLabelText("Form");
    fireEvent.change(formSelect, { target: { value: "Form A" } });
    fireEvent.change(await screen.findByLabelText("Field"), {
      target: { value: "Status" },
    });
    fireEvent.input(screen.getByLabelText("Value"), {
      target: { value: "open" },
    });
    fireEvent.change(formSelect, { target: { value: "Form B" } });

    const resetField = await screen.findByLabelText("Field");
    expect(resetField).toHaveValue("");
    fireEvent.click(
      screen.getByRole("button", { name: "Run advanced search" }),
    );
    await waitFor(() => {
      expect(sessionSql).not.toContain('"field_100"');
    });
  });

  it("does not save invalid advanced SQL when session validation fails", async () => {
    seedForm("default", {
      name: "Daily-Note",
      version: 1,
      template: "",
      fields: {},
      sql_relation: "daily_x2d_note",
    });
    let savedCalls = 0;
    let sessionSql = "";
    server.use(
      http.post(testApiUrl("/spaces/default/sql"), () => {
        savedCalls += 1;
        return HttpResponse.json({
          id: "should-not-exist",
          revision_id: "rev-failed",
        }, { status: 201 });
      }),
      http.post(
        testApiUrl("/spaces/default/sql-sessions"),
        async ({ request }) => {
          sessionSql = ((await request.json()) as { sql: string }).sql;
          return HttpResponse.json({
            id: "failed-session",
            status: "failed",
            error: "planner rejected query",
          }, { status: 201 });
        },
      ),
    );
    render(() => <SpaceSearchRoute />);
    fireEvent.click(screen.getByRole("button", { name: "Advanced search" }));
    await screen.findByRole("option", { name: "Daily-Note" });
    fireEvent.change(await screen.findByLabelText("Form"), {
      target: { value: "Daily-Note" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Run advanced search" }),
    );
    await waitFor(() => {
      expect(sessionSql).toContain('FROM "daily_x2d_note"');
      expect(screen.getByText("planner rejected query")).toBeInTheDocument();
      expect(savedCalls).toBe(0);
    });
  });

  it("binds string contains values with literal LIKE wildcards", async () => {
    seedForm("default", {
      name: "Notes",
      sql_relation: "notes",
      version: 1,
      template: "",
      fields: {
        Memo: { type: "string", required: false, sql_column: "field_100" },
      },
    });
    let sessionBody: {
      sql?: string;
      parameters?: Record<string, unknown>;
    } | null = null;
    server.use(
      http.post(
        testApiUrl("/spaces/default/sql-sessions"),
        async ({ request }) => {
          sessionBody = (await request.json()) as typeof sessionBody;
          return HttpResponse.json({
            id: "contains-session",
            status: "ready",
            error: null,
          }, { status: 201 });
        },
      ),
      http.post(
        testApiUrl("/spaces/default/sql"),
        () =>
          HttpResponse.json({
            id: "contains-saved",
            revision_id: "rev-contains",
          }, { status: 201 }),
      ),
    );
    render(() => <SpaceSearchRoute />);
    fireEvent.click(screen.getByRole("button", { name: "Advanced search" }));
    await screen.findByRole("option", { name: "Notes" });
    fireEvent.change(await screen.findByLabelText("Form"), {
      target: { value: "Notes" },
    });
    await screen.findByRole("option", { name: "Memo" });
    fireEvent.change(screen.getByLabelText("Field"), {
      target: { value: "Memo" },
    });
    fireEvent.change(screen.getByLabelText("Match"), {
      target: { value: "contains" },
    });
    fireEvent.input(screen.getByLabelText("Value"), {
      target: { value: "100%_match" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Run advanced search" }),
    );

    await waitFor(() => {
      expect(sessionBody?.parameters).toEqual({ search_0: "%100\\%\\_match%" });
      expect(sessionBody?.sql).toContain(
        "\"field_100\" ILIKE $search_0 ESCAPE '\\'",
      );
    });
  });

  it("keeps focus in a field-condition value while typing", async () => {
    seedForm("default", {
      name: "Meeting",
      sql_relation: "meeting",
      version: 1,
      template: "",
      fields: {
        memo: { type: "string", required: false, sql_column: "field_100" },
      },
    });
    render(() => <SpaceSearchRoute />);

    fireEvent.click(screen.getByRole("button", { name: "Advanced search" }));
    await screen.findByRole("option", { name: "Meeting" });
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "Meeting" },
    });
    await screen.findByRole("option", { name: "memo" });
    fireEvent.change(screen.getByLabelText("Field"), {
      target: { value: "memo" },
    });
    const value = screen.getByLabelText("Value");
    value.focus();
    fireEvent.input(value, { target: { value: "se" } });

    expect(value).toHaveFocus();
    expect(value).toHaveValue("se");
  });

  it("REQ-SRCH-005: saved history entries rerun directly or open variable input when needed", async () => {
    seedSqlEntry("default", {
      id: "saved-ready",
      name: "Ready history",
      sql: "SELECT * FROM entries WHERE title = 'Alpha'",
      variables: [],
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-02T00:00:00Z",
      revision_id: "rev-1",
    });
    seedSqlEntry("default", {
      id: "saved-vars",
      name: "Needs variables",
      sql: "SELECT * FROM entries WHERE title = {{title}}",
      variables: [{ type: "string", name: "title", description: "Title" }],
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-03T00:00:00Z",
      revision_id: "rev-2",
    });

    let sessionSqlBody: { sql?: string } | null = null;
    server.use(
      http.post(
        testApiUrl("/spaces/default/sql-sessions"),
        async ({ request }) => {
          sessionSqlBody = (await request.json()) as { sql?: string };
          return HttpResponse.json(
            { id: "history-session", status: "ready", error: null },
            { status: 201 },
          );
        },
      ),
    );

    render(() => <SpaceSearchRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: /Ready history/ }),
    );
    await waitFor(() => {
      expect(sessionSqlBody?.sql).toBe(
        "SELECT * FROM entries WHERE title = 'Alpha'",
      );
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/entries?session=history-session",
      );
    });

    navigateMock.mockReset();
    fireEvent.click(screen.getByRole("button", { name: /Needs variables/ }));
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/default/queries/saved-vars/variables",
    );
  });

  it("links the Assets facet to the Space asset workspace", () => {
    render(() => <SpaceSearchRoute />);

    expect(screen.getByRole("link", { name: /Assets/ })).toHaveAttribute(
      "href",
      "/spaces/default/assets",
    );
  });
});
