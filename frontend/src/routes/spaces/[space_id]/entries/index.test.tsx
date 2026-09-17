import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createMemo, createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";
import SpaceEntriesIndexPane from "./index";

const searchParams: Record<string, string> = {};
const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigate,
  useSearchParams: () => [searchParams, vi.fn()],
  A: (props: { href: string; children: unknown }) => (
    <a href={props.href}>{props.children}</a>
  ),
}));

function renderRoute(formsList: Form[] = [], spaceId = "default") {
  render(() => {
    const [forms] = createSignal(formsList);
    return (
      <EntriesRouteContext.Provider
        value={{
          spaceId: () => spaceId,
          forms: createMemo(forms),
          loadingForms: () => false,
          columnTypes: () => [],
          refetchForms: vi.fn(),
          entryStore: createEntryStore(() => spaceId),
          spaceStore: createSpaceStore(),
        }}
      >
        <SpaceEntriesIndexPane />
      </EntriesRouteContext.Provider>
    );
  });
}

const noteForm: Form = {
  name: "Notes",
  version: 1,
  template: "",
  fields: {
    title: { type: "string", required: true },
  },
};

describe("/spaces/:space_id/entries", () => {
  beforeEach(() => {
    setLocale("en");
    navigate.mockReset();
    for (const key of Object.keys(searchParams)) delete searchParams[key];
  });

  it("REQ-FE-037: loads the plain Entry list without redirecting", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/default/entries"),
        () =>
          HttpResponse.json([{
            id: "entry-1",
            title: "Entry one",
            updated_at: "2026-03-01T00:00:00Z",
            properties: {},
            tags: [],
          }]),
      ),
    );

    renderRoute();

    expect(await screen.findByRole("heading", { name: "Entries" }))
      .toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /Entry one/ }))
      .toBeInTheDocument();
    expect(screen.queryByTestId("redirect")).not.toBeInTheDocument();
  });

  it("provides a flat list with local filtering and title sorting", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/default/entries"),
        () =>
          HttpResponse.json([
            {
              id: "entry-1",
              title: "Zebra note",
              form: "Notes",
              updated_at: "2026-03-02T00:00:00Z",
              properties: {},
              tags: [],
            },
            {
              id: "entry-2",
              title: "Alpha note",
              form: "Notes",
              updated_at: "2026-03-01T00:00:00Z",
              properties: {},
              tags: [],
            },
          ]),
      ),
    );

    renderRoute();

    expect(await screen.findByRole("button", { name: /Zebra note/ }))
      .toBeInTheDocument();
    const filter = screen.getByRole("search");
    expect(filter).toBeInTheDocument();
    fireEvent.input(screen.getByLabelText("Filter entries"), {
      target: { value: "Alpha" },
    });
    expect(screen.queryByRole("button", { name: /Zebra note/ }))
      .not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Alpha note/ }))
      .toBeInTheDocument();

    fireEvent.input(screen.getByLabelText("Filter entries"), {
      target: { value: "" },
    });
    fireEvent.change(screen.getByLabelText("Sort entries"), {
      target: { value: "title" },
    });
    expect(
      [...document.querySelectorAll(".entryRowTitle")].map((node) =>
        node.textContent
      ),
    ).toEqual(["Alpha note", "Zebra note"]);
    fireEvent.change(screen.getByLabelText("Sort entries"), {
      target: { value: "updated" },
    });
    expect(
      [...document.querySelectorAll(".entryRowTitle")].map((node) =>
        node.textContent
      ),
    ).toEqual(["Zebra note", "Alpha note"]);
    expect(document.querySelector(".entryRow")).toBeInTheDocument();
    expect(document.querySelector(".entryRow .ui-card")).toBeNull();
  });

  it("REQ-FE-054: keeps the dedicated SQL session result route", async () => {
    searchParams.session = "session-1";
    server.use(
      http.get(
        testApiUrl("/spaces/default/sql-sessions/session-1"),
        () =>
          HttpResponse.json({
            id: "session-1",
            space_id: "default",
            sql_id: "query-1",
            sql: "SELECT 1",
            status: "ready",
            created_at: "2026-03-01T00:00:00Z",
            expires_at: "2026-03-01T01:00:00Z",
          }),
      ),
      http.get(
        testApiUrl("/spaces/default/sql-sessions/session-1/rows"),
        () =>
          HttpResponse.json({
            rows: [{
              _ugoite_id: "query-entry",
              _ugoite_title: "Query Entry",
              _ugoite_updated_at: 1772960822.056,
              field_100: "Active",
            }],
            offset: 0,
            limit: 24,
            total_count: 1,
          }),
      ),
    );

    renderRoute();

    const expectedDate = new Date(1772960822.056 * 1000).toLocaleDateString();
    expect(await screen.findByText("Query Entry")).toBeInTheDocument();
    expect(await screen.findByText(`Updated ${expectedDate}`))
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Query Entry/ }));
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/entries/query-entry",
    );
  });

  it("shows an explicit error for SQL rows that are not Entry projections", async () => {
    searchParams.session = "session-1";
    server.use(
      http.get(
        testApiUrl("/spaces/default/sql-sessions/session-1"),
        () =>
          HttpResponse.json({
            id: "session-1",
            status: "ready",
          }),
      ),
      http.get(
        testApiUrl("/spaces/default/sql-sessions/session-1/rows"),
        () =>
          HttpResponse.json({
            rows: [{ field_100: "Active" }],
            offset: 0,
            limit: 24,
            total_count: 1,
          }),
      ),
    );

    renderRoute();

    expect(
      await screen.findByText(/SQL session result is not an Entry projection/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Untitled/ }))
      .not.toBeInTheDocument();
    expect(navigate).not.toHaveBeenCalled();
  });

  it("returns to the Forms workspace when clearing SQL results", async () => {    searchParams.session = "session-1";
    server.use(
      http.get(
        testApiUrl("/spaces/default/sql-sessions/session-1"),
        () =>
          HttpResponse.json({
            id: "session-1",
            space_id: "default",
            sql_id: "query-1",
            sql: "SELECT 1",
            status: "ready",
            created_at: "2026-03-01T00:00:00Z",
            expires_at: "2026-03-01T01:00:00Z",
          }),
      ),
      http.get(
        testApiUrl("/spaces/default/sql-sessions/session-1/rows"),
        () =>
          HttpResponse.json({
            rows: [],
            offset: 0,
            limit: 24,
            total_count: 0,
          }),
      ),
    );
    renderRoute();
    expect(await screen.findByText("No entries found.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Clear query" }));

    expect(navigate).toHaveBeenCalledWith("/spaces/default/forms");
  });

  it("form scope: uses the Form name as heading with a back link and preselected New Entry", async () => {
    searchParams.form = "Notes";
    let queryBody: { filter?: Record<string, unknown> } | undefined;
    server.use(
      http.get(
        testApiUrl("/spaces/default/entries"),
        () =>
          HttpResponse.json([{
            id: "other-1",
            title: "Other form entry",
            form: "Projects",
            updated_at: "2026-03-01T00:00:00Z",
            properties: {},
            tags: [],
          }]),
      ),
      http.post(
        testApiUrl("/spaces/default/query"),
        async ({ request }) => {
          queryBody = await request.json() as {
            filter?: Record<string, unknown>;
          };
          return HttpResponse.json([{
            id: "scoped-1",
            title: "Scoped note",
            form: "Notes",
            updated_at: "2026-03-01T00:00:00Z",
            properties: {},
            tags: [],
          }]);
        },
      ),
    );

    renderRoute([noteForm]);

    expect(await screen.findByRole("heading", { name: "Notes" }))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Forms" }))
      .toHaveAttribute("href", "/spaces/default/forms");
    expect(await screen.findByRole("button", { name: /Scoped note/ }))
      .toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Other form entry/ })).not
      .toBeInTheDocument();
    expect(queryBody?.filter).toMatchObject({ form: "Notes" });

    fireEvent.click(screen.getByRole("button", { name: "New entry" }));
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/entries/new?form=Notes",
    );
  });

  it("form scope: hides New Entry for reserved metadata Forms", async () => {
    searchParams.form = "SQL";
    server.use(
      http.post(
        testApiUrl("/spaces/default/query"),
        () => HttpResponse.json([]),
      ),
    );

    renderRoute([]);

    expect(await screen.findByRole("heading", { name: "SQL" }))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Forms" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New entry" })).not
      .toBeInTheDocument();
    expect(await screen.findByText("No entries found.")).toBeInTheDocument();
  });

  it("form scope: shows server query failures instead of entries", async () => {
    searchParams.form = "Notes";
    server.use(
      http.post(
        testApiUrl("/spaces/default/query"),
        () => HttpResponse.json({ detail: "Error" }, { status: 500 }),
      ),
    );

    renderRoute([noteForm]);

    expect(await screen.findByRole("heading", { name: "Notes" }))
      .toBeInTheDocument();
    await waitFor(() =>
      expect(document.querySelector(".ui-text-danger")).toBeInTheDocument()
    );
    expect(screen.queryByRole("button", { name: /Scoped note/ })).not
      .toBeInTheDocument();
  });

  it("form scope: localizes the back link in Japanese", async () => {
    setLocale("ja");
    searchParams.form = "Notes";
    server.use(
      http.post(
        testApiUrl("/spaces/default/query"),
        () => HttpResponse.json([]),
      ),
    );

    renderRoute([noteForm]);

    expect(await screen.findByRole("link", { name: "フォームへ戻る" }))
      .toBeInTheDocument();
  });

  it("prefers the SQL session over the route form when both are present", async () => {
    searchParams.session = "session-1";
    searchParams.form = "Notes";
    let formQueried = false;
    server.use(
      http.get(
        testApiUrl("/spaces/default/sql-sessions/session-1"),
        () =>
          HttpResponse.json({
            id: "session-1",
            space_id: "default",
            sql_id: "query-1",
            sql: "SELECT 1",
            status: "ready",
            created_at: "2026-03-01T00:00:00Z",
            expires_at: "2026-03-01T01:00:00Z",
          }),
      ),
      http.get(
        testApiUrl("/spaces/default/sql-sessions/session-1/rows"),
        () =>
          HttpResponse.json({
            rows: [{
              _ugoite_id: "session-entry",
              _ugoite_title: "Session Entry",
              _ugoite_updated_at: 1772960822.056,
            }],
            offset: 0,
            limit: 24,
            total_count: 1,
          }),
      ),
      http.post(
        testApiUrl("/spaces/default/query"),
        () => {
          formQueried = true;
          return HttpResponse.json([]);
        },
      ),
    );

    renderRoute([noteForm]);

    expect(await screen.findByRole("heading", { name: "Query Results" }))
      .toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /Session Entry/ }))
      .toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Notes" })).not
      .toBeInTheDocument();
    expect(formQueried).toBe(false);
  });

  it("form scope: shows a helpful hint for an unknown form name", async () => {
    searchParams.form = "Missing";
    server.use(
      http.post(
        testApiUrl("/spaces/default/query"),
        () => HttpResponse.json([]),
      ),
    );

    renderRoute([noteForm]);

    expect(await screen.findByRole("heading", { name: "Missing" }))
      .toBeInTheDocument();
    expect(await screen.findByText(/No such form “Missing”/))
      .toBeInTheDocument();
    expect(screen.queryByText("No entries found.")).not.toBeInTheDocument();
  });

  it("encodes Space path segments in Entry navigation targets", async () => {
    const spaceId = "space/with space";
    let requestedPath = "";
    server.use(
      http.get(
        testApiUrl("/spaces/:spaceId/entries"),
        ({ request }) => {
          requestedPath = new URL(request.url).pathname;
          return HttpResponse.json([{
            id: "entry-1",
            title: "Entry one",
            updated_at: "2026-03-01T00:00:00Z",
            properties: {},
            tags: [],
          }]);
        },
      ),
    );

    renderRoute([], spaceId);

    fireEvent.click(await screen.findByRole("button", { name: /Entry one/ }));
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/space%2Fwith%20space/entries/entry-1",
    );
    // The store keeps the logical Space ID: the API client applies its own
    // single path encoding, so the request path carries exactly one level
    // of encoding and never the navigation-encoded string verbatim twice.
    expect(requestedPath).toContain("/spaces/space%2Fwith%20space/entries");
    expect(requestedPath).not.toContain("%25");
  });

  it("encodes Space path segments in the Forms back link and New Entry target", async () => {
    searchParams.form = "My Form";
    server.use(
      http.post(
        testApiUrl("/spaces/:spaceId/query"),
        () => HttpResponse.json([]),
      ),
    );

    renderRoute([noteForm], "space/with space");

    expect(await screen.findByRole("heading", { name: "My Form" }))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Forms" }))
      .toHaveAttribute("href", "/spaces/space%2Fwith%20space/forms");

    fireEvent.click(screen.getByRole("button", { name: "New entry" }));
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/space%2Fwith%20space/entries/new?form=My%20Form",
    );
  });
});
