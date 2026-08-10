import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceQueryVariablesRoute from "./variables";

const { navigateMock, sqlGetMock, sessionCreateMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  sqlGetMock: vi.fn(),
  sessionCreateMock: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default", query_id: "saved-vars" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  sqlApi: { get: sqlGetMock },
  sqlSessionApi: { create: sessionCreateMock },
}));

describe("/spaces/:space_id/queries/:query_id/variables", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    sqlGetMock.mockResolvedValue({
      id: "saved-vars",
      name: "Needs variables",
      kind: "user-query",
      sql:
        "SELECT * FROM form_entry WHERE _ugoite_title = {{title}} AND enabled = $enabled AND count = $count AND score = $score AND day = $day AND happened = $happened AND optional = $optional ORDER BY _ugoite_id",
      variables: [
        { type: "string", name: "title", description: "Title" },
        { type: "boolean", name: "enabled", description: "Enabled" },
        { type: "integer", name: "count", description: "Count" },
        { type: "float", name: "score", description: "Score" },
        { type: "date", name: "day", description: "Day" },
        { type: "timestamp", name: "happened", description: "Happened" },
        { type: "string", name: "optional", description: "Optional" },
      ],
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-02T00:00:00Z",
      revision_id: "rev-1",
    });
    sessionCreateMock.mockResolvedValue({
      id: "variable-session",
      status: "ready",
      error: null,
    });
  });

  it("runs with typed parameters and opens the shared result surface", async () => {
    render(() => <SpaceQueryVariablesRoute />);

    await screen.findByPlaceholderText("Title");
    for (
      const [placeholder, value] of [
        ["Title", "Alpha"],
        ["Enabled", "true"],
        ["Count", "3"],
        ["Score", "1.5"],
        ["Day", "2026-08-10"],
        ["Happened", "2026-08-10T12:34:56Z"],
      ]
    ) {
      fireEvent.input(screen.getByPlaceholderText(placeholder), {
        target: { value },
      });
    }
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => {
      expect(sessionCreateMock).toHaveBeenCalledWith(
        "default",
        "SELECT * FROM form_entry WHERE _ugoite_title = $title AND enabled = $enabled AND count = $count AND score = $score AND day = $day AND happened = $happened AND optional = $optional ORDER BY _ugoite_id",
        {
          title: "Alpha",
          enabled: true,
          count: 3,
          score: 1.5,
          day: "2026-08-10",
          happened: "2026-08-10T12:34:56Z",
          optional: null,
        },
        {
          title: "string",
          enabled: "boolean",
          count: "integer",
          score: "float",
          day: "date",
          happened: "timestamp",
          optional: "string",
        },
      );
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/entries?session=variable-session",
      );
    });
  });
});
