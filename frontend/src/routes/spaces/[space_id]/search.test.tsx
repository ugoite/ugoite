import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
    let metadataSql: string | null = null;
    server.use(
      http.get(
        testApiUrl("/spaces/default/search"),
        () => HttpResponse.json([{ id: entry.id }]),
      ),
      http.get(
        testApiUrl("/spaces/default/entries"),
        () =>
          new HttpResponse(
            "bulk entry list should not be used for keyword search",
            {
              status: 500,
            },
          ),
      ),
      http.post(
        testApiUrl("/spaces/default/sql-sessions"),
        async ({ request }) => {
          const body = (await request.json()) as { sql?: string };
          metadataSql = body.sql ?? null;
          return HttpResponse.json(
            { id: "keyword-session", status: "ready", error: null },
            { status: 201 },
          );
        },
      ),
      http.get(
        testApiUrl("/spaces/default/sql-sessions/keyword-session/rows"),
        () =>
          HttpResponse.json({
            rows: [record],
            offset: 0,
            limit: 1,
            total_count: 1,
          }),
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
    expect(metadataSql).toBe(
      "SELECT * FROM entries WHERE id IN ('entry-1') LIMIT 1",
    );
  });

  it("REQ-SRCH-005: advanced search compiles filters into saved SQL and runs a shared session", async () => {
    const meetingForm: Form = {
      name: "Meeting",
      version: 1,
      template: "# Meeting\n\n## Status\n",
      fields: {
        Status: { type: "string", required: false },
      },
    };
    seedForm("default", meetingForm);

    let savedSqlBody: {
      name?: string | null;
      kind?: string;
      metadata?: unknown;
      sql?: string;
    } | null = null;
    let sessionSqlBody: { sql?: string } | null = null;

    server.use(
      http.post(testApiUrl("/spaces/default/sql"), async ({ request }) => {
        savedSqlBody = (await request.json()) as {
          name?: string | null;
          kind?: string;
          metadata?: unknown;
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
          sessionSqlBody = (await request.json()) as { sql?: string };
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
    fireEvent.input(screen.getByLabelText("Tags (comma-separated)"), {
      target: { value: "project" },
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
      expect(savedSqlBody?.kind).toBe("search-history");
      expect(savedSqlBody?.metadata).toEqual({
        searchCriteria: {
          formName: "Meeting",
          tags: ["project"],
          updatedFrom: "2025-03-01",
          updatedTo: "2025-03-03",
          fieldConditions: [{
            field: "Status",
            operator: "equals",
            value: "Active",
          }],
        },
      });
      expect(savedSqlBody?.sql).toBe(
        "SELECT * FROM entries WHERE form = 'Meeting' AND tags = 'project' AND updated_at >= 1740787200 AND updated_at <= 1741046399 AND properties.\"Status\" = 'Active' ORDER BY updated_at DESC LIMIT 50",
      );
      expect(sessionSqlBody?.sql).toBe(savedSqlBody?.sql);
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/entries?session=advanced-session",
      );
    });
  });

  it("keeps focus in a field-condition value while typing", async () => {
    seedForm("default", {
      name: "Meeting",
      version: 1,
      template: "",
      fields: { memo: { type: "string", required: false } },
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
      await screen.findByRole("button", { name: /Advanced search - form: Meeting/ }),
    ).toBeInTheDocument();

    setLocale("ja");
    expect(
      screen.getByRole("button", { name: /詳細検索 - フォーム: Meeting/ }),
    ).toBeInTheDocument();
  });
});
