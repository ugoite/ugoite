import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceSqlRoute from "./index";
import { formatDateLabel } from "~/lib/date-format";
import { setLocale } from "~/lib/i18n";
import { sqlApi } from "~/lib/ugoite-client";
import type { SqlEntry } from "~/lib/types";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  sqlApi: { list: vi.fn().mockResolvedValue([]) },
}));

describe("/spaces/:space_id/sql", () => {
  beforeEach(() => {
    setLocale("en");
    vi.mocked(sqlApi.list).mockResolvedValue([]);
  });

  it("REQ-FE-061: saved SQL route provides the v5 list and create action", async () => {
    render(() => <SpaceSqlRoute />);

    expect(screen.getByRole("heading", { name: "Saved SQL" }))
      .toBeInTheDocument();
    expect(screen.getByText("No saved SQL", { exact: true }))
      .toBeInTheDocument();
    expect(
      screen.getByText("Create a query to reuse it here.", { exact: true }),
    )
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "SQL" })).toHaveAttribute(
      "href",
      "/spaces/default/queries/new",
    );
  });

  it("REQ-FE-061: saved SQL route shows a canonical load error", async () => {
    vi.mocked(sqlApi.list).mockRejectedValueOnce(new Error("backend down"));

    render(() => <SpaceSqlRoute />);

    expect(
      await screen.findByText("Failed to load saved SQL.", { exact: false }),
    ).toBeInTheDocument();
  });

  it("REQ-FE-061: saved SQL route links a saved query to its detail route", async () => {
    vi.mocked(sqlApi.list).mockResolvedValueOnce([{
      id: "saved/query",
      name: "Recent Query",
      kind: "user-query",
      metadata: null,
      sql: "SELECT 1",
      variables: [],
      created_at: "2026-03-01T00:00:00Z",
      updated_at: "2026-03-02T00:00:00Z",
      revision_id: "rev-1",
    }]);

    render(() => <SpaceSqlRoute />);

    expect(await screen.findByRole("link", { name: /Recent Query/ }))
      .toHaveAttribute("href", "/spaces/default/sql/saved%2Fquery");
  });

  it("uses the selected locale for structured history names and dates", async () => {
    const entry: SqlEntry = {
      id: "history-1",
      name: null,
      kind: "search-history",
      metadata: {
        searchCriteria: {
          formName: "Incident",
          tags: [],
          updatedFrom: "",
          updatedTo: "",
          fieldConditions: [],
        },
      },
      sql: "SELECT * FROM entries",
      variables: [],
      created_at: "2026-07-30T00:00:00Z",
      updated_at: "2026-07-31T00:00:00Z",
      revision_id: "rev-1",
    };
    vi.mocked(sqlApi.list).mockResolvedValue([entry]);

    render(() => <SpaceSqlRoute />);
    expect(await screen.findByText("Advanced search - form: Incident"))
      .toBeInTheDocument();
    expect(screen.getByText(formatDateLabel(entry.updated_at)))
      .toBeInTheDocument();

    cleanup();
    setLocale("ja");
    render(() => <SpaceSqlRoute />);
    expect(await screen.findByText("詳細検索 - フォーム: Incident"))
      .toBeInTheDocument();
    expect(screen.getByText(formatDateLabel(entry.updated_at)))
      .toBeInTheDocument();
  });
});
