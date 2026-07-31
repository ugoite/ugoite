import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { http, HttpResponse } from "msw";
import SpaceSqlDetailRoute from "./[sql_id]";
import { resetMockData, seedSpace, seedSqlEntry } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Space } from "~/lib/types";
import { testApiUrl } from "~/test/http-origin";

const navigateMock = vi.fn();
const entryRelation = "form_00000000000000000000000000000001";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default", sql_id: "saved-query" }),
}));

vi.mock("~/components/SpaceShell", () => ({
  SpaceShell: (
    props: { children: unknown; spaceId: string; activeNavigation?: string },
  ) => (
    <div
      data-space-id={props.spaceId}
      data-active-navigation={props.activeNavigation}
    >
      {props.children}
    </div>
  ),
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
    id: "default",
    name: "Default",
    created_at: "2025-01-01T00:00:00Z",
  };

  beforeEach(() => {
    navigateMock.mockReset();
    resetMockData();
    seedSpace(testSpace);
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

    const { container } = render(() => <SpaceSqlDetailRoute />);

    expect(await screen.findByRole("heading", { name: "Recent Search" }))
      .toBeInTheDocument();
    expect(screen.getByText("Search / Saved SQL")).toBeInTheDocument();
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
        "/spaces/default/queries/saved-query/variables",
      );
    expect(screen.getByRole("link", { name: "Back to Saved SQL" }))
      .toHaveAttribute(
        "href",
        "/spaces/default/sql",
      );
    expect(screen.getByRole("link", { name: "Open Search" })).toHaveAttribute(
      "href",
      "/spaces/default/search",
    );
    expect(container.firstElementChild).toHaveAttribute(
      "data-space-id",
      "default",
    );
    expect(container.firstElementChild).toHaveAttribute(
      "data-active-navigation",
      "search",
    );
  });

  it("REQ-FE-062: saved SQL detail runs variable-free queries through SQL sessions", async () => {
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
    server.use(
      http.post(
        testApiUrl("/spaces/default/sql-sessions"),
        async ({ request }) => {
          const body = (await request.json()) as { sql?: string };
          expect(body.sql).toBe(
            `SELECT * FROM "${entryRelation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 1`,
          );
          return HttpResponse.json(
            { id: "session-1", status: "ready", error: null },
            { status: 201 },
          );
        },
      ),
    );

    render(() => <SpaceSqlDetailRoute />);
    fireEvent.click(await screen.findByRole("button", { name: "Run Query" }));

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/entries?session=session-1",
      );
    });
  });
});
