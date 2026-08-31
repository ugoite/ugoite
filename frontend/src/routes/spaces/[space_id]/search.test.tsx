import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { http, HttpResponse } from "msw";
import SpaceSearchRoute from "./search";
import {
  resetMockData,
  seedForm,
  seedSpace,
  seedSqlEntry,
} from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Form, KeywordSearchResult, Space } from "~/lib/types";
import { testApiUrl } from "~/test/http-origin";
import { setLocale } from "~/lib/i18n";

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

async function test_search_req_srch_004_keyword_first_route() {
  const record: KeywordSearchResult = {
    id: "entry-1",
    title: "Alpha Entry",
    created_at: "2025-01-01T00:00:00Z",
    updated_at: "2025-01-02T00:00:00Z",
  };
  let entryListCalls = 0;
  let sqlSessionCalls = 0;
  server.use(
    http.get(
      testApiUrl("/spaces/default/search"),
      () => HttpResponse.json([record]),
    ),
    http.get(
      testApiUrl("/spaces/default/entries"),
      () => {
        entryListCalls += 1;
        return HttpResponse.json([]);
      },
    ),
    http.post(
      testApiUrl("/spaces/default/sql-sessions"),
      () => {
        sqlSessionCalls += 1;
        return HttpResponse.json(
          { detail: "Quick search must not create a SQL session" },
          { status: 500 },
        );
      },
    ),
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
}

async function test_search_req_srch_005_advanced_search_compiles() {
  const meetingForm: Form = {
    name: "Meeting",
    version: 1,
    template: "# Meeting\n\n## Status\n",
    fields: {
      Status: { type: "string", required: false, sql_column: "field_100" },
    },
    sql_relation: "form_meeting",
  };
  seedForm("default", meetingForm);

  let savedSqlBody: { name?: string | null; sql?: string } | null = null;
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
        name?: string | null;
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
    expect(savedSqlBody?.name).toBeNull();
    expect(savedSqlBody?.sql).toBe(
      "SELECT * FROM \"form_meeting\" WHERE _ugoite_updated_at >= TIMESTAMP '2025-03-01 00:00:00Z' AND _ugoite_updated_at < TIMESTAMP '2025-03-04 00:00:00Z' AND \"field_100\" = 'Active' ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50",
    );
    expect(sessionSqlBody?.sql).toContain('"field_100" = $search_2');
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
}

async function test_search_req_srch_005_history_reuse() {
  const relation = "form_entry";
  seedForm("default", {
    name: "Entry",
    sql_relation: relation,
    version: 1,
    template: "# Entry\n",
    fields: {},
  });
  seedSqlEntry("default", {
    id: "saved-ready",
    name: "Ready history",
    sql:
      `SELECT * FROM "${relation}" WHERE _ugoite_title = 'Alpha' ORDER BY _ugoite_id`,
    variables: [],
    created_at: "2025-01-01T00:00:00Z",
    updated_at: "2025-01-02T00:00:00Z",
    revision_id: "rev-1",
  });
  seedSqlEntry("default", {
    id: "saved-vars",
    name: "Needs variables",
    sql:
      `SELECT * FROM "${relation}" WHERE _ugoite_title = $title ORDER BY _ugoite_id`,
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
      `SELECT * FROM "${relation}" WHERE _ugoite_title = 'Alpha' ORDER BY _ugoite_id`,
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
}

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
    setLocale("en");
  });

  afterEach(() => setLocale("en"));

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

  it(
    "REQ-SRCH-004: runs a direct keyword search and renders matching entries",
    test_search_req_srch_004_keyword_first_route,
  );

  it(
    "REQ-SRCH-005: advanced search compiles filters into saved SQL and runs a shared session",
    test_search_req_srch_005_advanced_search_compiles,
  );

  it("keeps focus in a field-condition value while typing", async () => {
    seedForm("default", {
      name: "Meeting",
      version: 1,
      template: "",
      fields: {
        memo: { type: "string", required: false, sql_column: "field_100" },
      },
      sql_relation: "form_meeting",
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

  it("restores field-specific search input controls and requires a form", async () => {
    seedForm("default", {
      name: "Typed fields",
      version: 1,
      template: "",
      fields: {
        enabled: { type: "boolean", required: false, sql_column: "enabled" },
        count: { type: "integer", required: false, sql_column: "count" },
        score: { type: "number", required: false, sql_column: "score" },
        due: { type: "date", required: false, sql_column: "due" },
        happened: {
          type: "timestamp",
          required: false,
          sql_column: "happened",
        },
      },
      sql_relation: "form_typed_fields",
    });

    render(() => <SpaceSearchRoute />);
    fireEvent.click(screen.getByRole("button", { name: "Advanced search" }));
    expect(screen.getByRole("option", { name: "Select a form" }))
      .toBeInTheDocument();
    await screen.findByRole("option", { name: "Typed fields" });
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "Typed fields" },
    });
    await screen.findByRole("option", { name: /enabled/ });

    const cases = [
      ["enabled", "text", "true or false"],
      ["count", "number", "42"],
      ["score", "number", "3.14"],
      ["due", "date", "YYYY-MM-DD"],
      ["happened", "datetime-local", "YYYY-MM-DDTHH:mm"],
    ] as const;
    for (const [field, type, placeholder] of cases) {
      await screen.findByRole("option", { name: new RegExp(field) });
      fireEvent.change(screen.getByLabelText("Field"), {
        target: { value: field },
      });
      await waitFor(() => {
        const input = screen.getByLabelText("Value");
        expect(input).toHaveAttribute("type", type);
        expect(input).toHaveAttribute("placeholder", placeholder);
      });
    }

    fireEvent.change(screen.getByLabelText("Form"), { target: { value: "" } });
    fireEvent.click(
      screen.getByRole("button", { name: "Run advanced search" }),
    );
    expect(await screen.findByText(/Choose a Form/)).toBeInTheDocument();
  });

  it(
    "REQ-SRCH-005: saved history entries rerun directly or open variable input when needed",
    test_search_req_srch_005_history_reuse,
  );

  it("links the Assets facet to the Space asset workspace", () => {
    render(() => <SpaceSearchRoute />);

    expect(screen.getByRole("link", { name: /Assets/ })).toHaveAttribute(
      "href",
      "/spaces/default/assets",
    );
  });

  it("REQ-FE-044: keeps search controls and state messages in Japanese", () => {
    setLocale("ja");

    render(() => <SpaceSearchRoute />);

    expect(screen.getByRole("heading", { name: "検索" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "クイック検索" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "詳細検索" }))
      .toBeInTheDocument();
    expect(screen.getByLabelText("検索キーワード")).toHaveAttribute(
      "placeholder",
      "タイトル、フィールド、タグ、本文からエントリを検索",
    );
    expect(screen.getByText("キーワード検索結果")).toBeInTheDocument();
    expect(screen.getByText("検索履歴")).toBeInTheDocument();
    expect(screen.queryByText("Search")).not.toBeInTheDocument();
  });

  it("REQ-FE-044: renders generated history in the selected locale", async () => {
    seedSqlEntry("default", {
      id: "generated-history",
      name: null,
      kind: "search-history",
      metadata: {
        searchCriteria: {
          formName: "Meeting",
          tags: ["project"],
          updatedFrom: "",
          updatedTo: "",
          fieldConditions: [],
        },
      },
      sql: "SELECT * FROM entries WHERE form = 'Meeting'",
      variables: [],
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-02T00:00:00Z",
      revision_id: "rev-generated",
    });

    render(() => <SpaceSearchRoute />);
    expect(
      await screen.findByRole("button", {
        name: /Advanced search - form: Meeting/,
      }),
    ).toBeInTheDocument();

    setLocale("ja");
    expect(
      screen.getByRole("button", { name: /詳細検索 - フォーム: Meeting/ }),
    ).toBeInTheDocument();
  });
});
