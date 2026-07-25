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
  Navigate: (props: { href: string }) => (
    <a data-testid="redirect" href={props.href}>Redirect</a>
  ),
  A: (props: { href: string; children: unknown }) => (
    <a href={props.href}>{props.children}</a>
  ),
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

  it("redirects the duplicate plain Entry list to the Forms workspace", () => {
    renderRoute();
    expect(screen.getByTestId("redirect")).toHaveAttribute(
      "href",
      "/spaces/default/forms",
    );
  });

  it("REQ-FE-054: keeps the dedicated SQL session result route", async () => {
    searchParams.session = "session-1";
    server.use(
      http.get(testApiUrl("/spaces/default/sql-sessions/session-1"), () =>
        HttpResponse.json({
          id: "session-1",
          space_id: "default",
          sql_id: "query-1",
          sql: "SELECT 1",
          status: "ready",
          created_at: "2026-03-01T00:00:00Z",
          expires_at: "2026-03-01T01:00:00Z",
        })),
      http.get(
        testApiUrl("/spaces/default/sql-sessions/session-1/rows"),
        () =>
          HttpResponse.json({
            rows: [{
              id: "query-entry",
              title: "Query Entry",
              form: "Meeting",
              updated_at: 1772960822.056,
              properties: {},
              tags: [],
              links: [],
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
  });

  it("returns to the Forms workspace when clearing SQL results", async () => {
    searchParams.session = "session-1";
    server.use(
      http.get(testApiUrl("/spaces/default/sql-sessions/session-1"), () =>
        HttpResponse.json({
          id: "session-1",
          space_id: "default",
          sql_id: "query-1",
          sql: "SELECT 1",
          status: "ready",
          created_at: "2026-03-01T00:00:00Z",
          expires_at: "2026-03-01T01:00:00Z",
        })),
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
