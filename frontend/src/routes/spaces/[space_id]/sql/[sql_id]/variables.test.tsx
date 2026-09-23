import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceQueryVariablesRoute from "./variables";

const { navigateMock, sqlGetMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  sqlGetMock: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default", sql_id: "saved-vars" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  sqlApi: { get: sqlGetMock },
}));

describe("/spaces/:space_id/sql/:sql_id/variables", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    sqlGetMock.mockResolvedValue({
      id: "saved-vars",
      name: "Needs variables",
      kind: "user-query",
      sql:
        "SELECT * FROM form_entry WHERE Body = {{title}} AND enabled = $enabled AND count = $count AND score = $score AND day = $day AND happened = $happened AND optional = $optional ORDER BY _ugoite_id",
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
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/sql/saved-vars/run",
        {
          state: {
            parameters: {
              title: "Alpha",
              enabled: true,
              count: 3,
              score: 1.5,
              day: "2026-08-10",
              happened: "2026-08-10T12:34:56Z",
              optional: null,
            },
            parameterTypes: {
              title: "string",
              enabled: "boolean",
              count: "integer",
              score: "float",
              day: "date",
              happened: "timestamp",
              optional: "string",
            },
          },
        },
      );
    });
  });

  it("PR6: backs to the saved query once with typed variable inputs", async () => {
    render(() => <SpaceQueryVariablesRoute />);

    const back = await screen.findByRole("link", { name: "Back to Saved SQL" });
    expect(back).toHaveAttribute(
      "href",
      "/spaces/default/sql/saved-vars",
    );
    expect(screen.getAllByRole("link", { name: "Back to Saved SQL" }))
      .toHaveLength(1);
    // Typed variables stay on the normal path without raw JSON.
    expect(await screen.findByLabelText(/title/)).toBeInTheDocument();
    expect(screen.queryByText("{")).not.toBeInTheDocument();
  });
});
