import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { createMemo, createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";
import { setLocale } from "~/lib/i18n";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";
import SpaceEntriesIndexPane from "./index";

const searchParams: Record<string, string> = {};
const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigate,
  useSearchParams: () => [searchParams, vi.fn()],
}));

function renderRoute() {
  render(() => {
    const [forms] = createSignal([]);
    return (
      <EntriesRouteContext.Provider
        value={{
          spaceId: () => "default",
          forms: createMemo(forms),
          loadingForms: () => false,
          columnTypes: () => [],
          refetchForms: vi.fn(),
          entryStore: createEntryStore(() => "default"),
          spaceStore: createSpaceStore(),
        }}
      >
        <SpaceEntriesIndexPane />
      </EntriesRouteContext.Provider>
    );
  });
}

describe("/spaces/:space_id/entries", () => {
  beforeEach(() => {
    setLocale("en");
    navigate.mockReset();
    for (const key of Object.keys(searchParams)) delete searchParams[key];
  });

  it("renders the plain Entries index and loads its entries", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/default/entries"),
        () =>
          HttpResponse.json([{
            id: "entry-1",
            title: "Existing entry",
            updated_at: "2026-03-01T00:00:00Z",
            properties: {},
            tags: [],
          }]),
      ),
    );

    renderRoute();

    expect(await screen.findByRole("heading", { name: "Entries" }))
      .toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /Existing entry/ }))
      .toBeInTheDocument();
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

  it("returns to the Forms workspace when clearing SQL results", async () => {
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
});
