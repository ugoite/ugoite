import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { delay, http, HttpResponse } from "msw";
import SpaceSearchRoute from "./search";
import { resetMockData, seedForm, seedSpace } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Form, KeywordSearchResult, Space } from "~/lib/types";
import { testApiUrl } from "~/test/http-origin";
import { setLocale } from "~/lib/i18n";
import { localInputToRfc3339Instant } from "~/lib/search-date";

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
    setLocale("en");
  });

  afterEach(() => setLocale("en"));

  it("REQ-SRCH-004: runs a direct keyword search and renders matching entries", async () => {
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
  });

  it("REQ-SRCH-005: advanced search sends logical criteria without SQL construction", async () => {
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

    let queryBody: {
      criteria?: {
        form?: string;
        updated_from?: string;
        updated_to?: string;
        conditions?: Array<
          { field?: string; operator?: string; value?: unknown }
        >;
        limit?: number;
      };
    } | null = null;
    let sqlSessionCalls = 0;
    let savedSqlCalls = 0;

    server.use(
      http.post(testApiUrl("/spaces/default/query"), async ({ request }) => {
        queryBody = (await request.json()) as typeof queryBody;
        return HttpResponse.json([
          {
            id: "entry-1",
            title: "Active Meeting",
            form: "Meeting",
            updated_at: "2025-03-02T00:00:00Z",
            properties: { Status: "Active" },
            tags: [],
          },
        ]);
      }),
      http.post(testApiUrl("/spaces/default/sql-sessions"), () => {
        sqlSessionCalls += 1;
        return HttpResponse.json(
          { detail: "Advanced search must not create a SQL session" },
          { status: 500 },
        );
      }),
      http.post(testApiUrl("/spaces/default/sql"), () => {
        savedSqlCalls += 1;
        return HttpResponse.json(
          { detail: "Advanced search must not save SQL" },
          { status: 500 },
        );
      }),
    );

    render(() => <SpaceSearchRoute />);

    fireEvent.click(screen.getByRole("button", { name: "Show filters" }));
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
      expect(queryBody?.criteria).toEqual({
        form: "Meeting",
        updated_from: localInputToRfc3339Instant("2025-03-01", "start"),
        updated_to: localInputToRfc3339Instant("2025-03-03", "end"),
        conditions: [{ field: "Status", operator: "equals", value: "Active" }],
        limit: 51,
      });
    });
    // No raw SQL construction on the advanced path.
    expect(JSON.stringify(queryBody)).not.toContain("SELECT");
    expect(JSON.stringify(queryBody)).not.toContain("field_100");
    expect(JSON.stringify(queryBody)).not.toContain("form_meeting");
    expect(sqlSessionCalls).toBe(0);
    expect(savedSqlCalls).toBe(0);

    expect(await screen.findByRole("button", { name: /Active Meeting/ }))
      .toBeInTheDocument();
    expect(screen.getByText("1 result")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Active Meeting/ }));
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/default/entries/entry-1",
    );
  });

  it("preserves wall-clock timestamp fields while converting instant fields", async () => {
    seedForm("default", {
      name: "Times",
      version: 1,
      template: "",
      fields: {
        LocalTime: { type: "timestamp", required: false },
        Instant: { type: "timestamp_tz", required: false },
      },
      sql_relation: "form_times",
    });
    const queryBodies: Array<{
      criteria?: {
        conditions?: Array<{ value?: unknown }>;
      };
    }> = [];
    server.use(
      http.post(testApiUrl("/spaces/default/query"), async ({ request }) => {
        queryBodies.push(await request.json() as typeof queryBodies[number]);
        return HttpResponse.json([]);
      }),
    );

    render(() => <SpaceSearchRoute />);
    fireEvent.click(screen.getByRole("button", { name: "Show filters" }));
    await screen.findByRole("option", { name: "Times" });
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "Times" },
    });
    await screen.findByRole("option", { name: "LocalTime" });
    fireEvent.change(screen.getByLabelText("Field"), {
      target: { value: "LocalTime" },
    });
    fireEvent.input(screen.getByLabelText("Value"), {
      target: { value: "2026-03-08T01:30" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Run advanced search" }),
    );

    await waitFor(() => expect(queryBodies).toHaveLength(1));
    expect(queryBodies[0]?.criteria?.conditions?.[0]?.value).toBe(
      "2026-03-08T01:30",
    );

    fireEvent.change(screen.getByLabelText("Field"), {
      target: { value: "Instant" },
    });
    fireEvent.input(screen.getByLabelText("Value"), {
      target: { value: "2026-03-08T01:30" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Run advanced search" }),
    );

    await waitFor(() => expect(queryBodies).toHaveLength(2));
    expect(queryBodies[1]?.criteria?.conditions?.[0]?.value).toBe(
      localInputToRfc3339Instant("2026-03-08T01:30"),
    );
  });

  it("advanced search disables unsupported fields with a reason and blocks execution", async () => {
    seedForm("default", {
      name: "Assets",
      version: 1,
      template: "",
      fields: {
        file: { type: "binary", required: false },
      },
      // Backend-provided relation; the advanced path must not consume it.
      sql_relation: "form_assets",
    });
    let queryCalls = 0;
    server.use(
      http.post(testApiUrl("/spaces/default/query"), () => {
        queryCalls += 1;
        return HttpResponse.json([]);
      }),
    );

    render(() => <SpaceSearchRoute />);
    fireEvent.click(screen.getByRole("button", { name: "Show filters" }));
    await screen.findByRole("option", { name: "Assets" });
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "Assets" },
    });
    const option = await screen.findByRole("option", { name: /file/ });
    expect(option).toBeDisabled();
    expect(option.textContent).toMatch(/unsupported|検索対象外/);
    fireEvent.change(screen.getByLabelText("Field"), {
      target: { value: "file" },
    });
    expect(
      await screen.findByText(
        /is not supported by Advanced search|は詳細検索に対応していません/,
      ),
    ).toBeInTheDocument();
    fireEvent.input(screen.getByLabelText("Value"), {
      target: { value: "x" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Run advanced search" }),
    );
    // The reason stays inline under the field and is also surfaced as the
    // action error; execution must still be blocked.
    expect(
      await screen.findAllByText(
        /is not supported by Advanced search|は詳細検索に対応していません/,
      ),
    ).toHaveLength(2);
    expect(queryCalls).toBe(0);
  });

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

    fireEvent.click(screen.getByRole("button", { name: "Show filters" }));
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
    fireEvent.click(screen.getByRole("button", { name: "Show filters" }));
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

  it("PR6: keeps one search experience with progressive filters and related links", () => {
    render(() => <SpaceSearchRoute />);

    // One experience: the keyword box is always visible; filters are
    // progressive disclosure, not a separate product tab.
    expect(screen.getByLabelText("Search keywords")).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Search" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Quick search" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Advanced search" }))
      .not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Assets" }))
      .toHaveAttribute(
        "href",
        "/spaces/default/assets",
      );
    expect(screen.getByRole("link", { name: "Saved SQL" }))
      .toHaveAttribute("href", "/spaces/default/sql");
    expect(screen.getByText(/advanced Search surface/i)).toBeInTheDocument();
    expect(screen.queryByText("Open SQL editor"))
      .not.toBeInTheDocument();
    expect(screen.queryByText("Search history")).not.toBeInTheDocument();
    expect(document.querySelector(".facet")).not.toBeInTheDocument();

    // Filters stay hidden until requested.
    expect(screen.queryByLabelText("Form")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Show filters" }),
    );
    expect(screen.getByLabelText("Form")).toBeInTheDocument();
    expect(document.querySelector(".searchCondition.ui-card"))
      .not.toBeInTheDocument();
  });

  it("PR6: renders result rows through RowList with full-row activation", async () => {
    const record: KeywordSearchResult = {
      id: "entry-9",
      title: "Row Entry",
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-02T00:00:00Z",
    };
    server.use(
      http.get(
        testApiUrl("/spaces/default/search"),
        () => HttpResponse.json([record]),
      ),
    );

    const { container } = render(() => <SpaceSearchRoute />);

    fireEvent.input(screen.getByLabelText("Search keywords"), {
      target: { value: "row" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));

    const row = await screen.findByRole("button", { name: /Row Entry/ });
    expect(row).toHaveClass("rowListMain");
    expect(container.querySelector(".rowList")).toBeInTheDocument();
    expect(container.querySelector(".searchResultRow")).toBeNull();
    fireEvent.click(row);
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/default/entries/entry-9",
    );
  });

  it("PR4: keeps previous results and the count visible during re-search", async () => {
    const alpha: KeywordSearchResult = {
      id: "entry-1",
      title: "Alpha Entry",
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-02T00:00:00Z",
    };
    const beta: KeywordSearchResult = {
      id: "entry-2",
      title: "Beta Entry",
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-03T00:00:00Z",
    };
    let calls = 0;
    server.use(
      http.get(testApiUrl("/spaces/default/search"), async () => {
        calls += 1;
        if (calls === 1) return HttpResponse.json([alpha]);
        await delay(150);
        return HttpResponse.json([beta]);
      }),
    );

    render(() => <SpaceSearchRoute />);

    fireEvent.input(screen.getByLabelText("Search keywords"), {
      target: { value: "first" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));
    expect(await screen.findByRole("button", { name: /Alpha Entry/ }))
      .toBeInTheDocument();
    expect(screen.getByText("1 result")).toBeInTheDocument();

    fireEvent.input(screen.getByLabelText("Search keywords"), {
      target: { value: "second" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));

    // Previous results and the count stay mounted while reloading; only
    // spinner indicators (role=status, sr-only label) signal the reload.
    expect(screen.getByRole("button", { name: /Alpha Entry/ }))
      .toBeInTheDocument();
    expect(screen.getByText("1 result")).toBeInTheDocument();
    // No visible loading copy: the submit button keeps its static label and
    // every "Searching entries..." string is sr-only inside a status spinner.
    expect(
      screen.getByRole("button", { name: "Search entries" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Searching..." })).not
      .toBeInTheDocument();
    for (
      const node of screen.getAllByText("Searching entries...")
    ) {
      expect(node).toHaveClass("ui-sr-only");
    }

    expect(await screen.findByRole("button", { name: /Beta Entry/ }))
      .toBeInTheDocument();
  });
  it("REQ-FE-044: keeps search controls and state messages in Japanese", () => {
    setLocale("ja");

    render(() => <SpaceSearchRoute />);

    expect(screen.getByRole("heading", { name: "検索" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "フィルターを表示" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "クイック検索" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "詳細検索" }))
      .not.toBeInTheDocument();
    expect(screen.getByLabelText("検索キーワード")).toHaveAttribute(
      "placeholder",
      "タイトル、フィールド、タグ、本文からエントリを検索",
    );
    // PR4: the visible label and result headings are removed; the label
    // association and sr-only headings remain for assistive technology.
    expect(document.querySelector('label[for="search-keywords"]'))
      .toHaveClass("ui-sr-only");
    expect(screen.getByText("キーワード検索結果")).toHaveClass("ui-sr-only");
    expect(screen.queryByText("検索履歴")).not.toBeInTheDocument();
    expect(screen.queryByText("Search")).not.toBeInTheDocument();
  });
});
