import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import type { JSX } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { compositionApi } from "~/lib/composition-api";
import { setLocale } from "~/lib/i18n";
import CompositionListRoute from "./index";

vi.mock("@solidjs/router", () => ({
  useParams: () => ({ space_id: "space-1" }),
  A: (props: { href: string; class?: string; children: JSX.Element }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
}));
vi.mock("~/lib/composition-api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("~/lib/composition-api")>();
  return {
    ...actual,
    compositionApi: { ...actual.compositionApi, list: vi.fn() },
  };
});

const emptyPage = {
  items: [],
  offset: 0,
  limit: 50,
  has_more: false,
};

describe("Composition list route", () => {
  beforeEach(() => {
    setLocale("en");
    vi.mocked(compositionApi.list).mockReset();
  });

  it("shows explicit loading and empty states", async () => {
    let resolve!: (value: typeof emptyPage) => void;
    vi.mocked(compositionApi.list).mockReturnValue(
      new Promise((done) => resolve = done),
    );
    render(() => <CompositionListRoute />);

    expect(screen.getByRole("status")).toHaveTextContent("Loading saved tools");
    resolve(emptyPage);
    expect(await screen.findByText("No saved tools")).toBeInTheDocument();
  });

  it("opens a row at the exact revision and pages within the bounded list", async () => {
    vi.mocked(compositionApi.list)
      .mockResolvedValueOnce({
        items: [{
          composition_id: "tool-1",
          revision_id: "revision-2",
          updated_at: 1772960822,
          name: "Monthly expenses",
          kind: "dashboard",
          format_version: 1,
          tags: [],
        }],
        offset: 0,
        limit: 50,
        has_more: true,
      })
      .mockResolvedValueOnce({ ...emptyPage, offset: 50 });
    render(() => <CompositionListRoute />);

    expect(await screen.findByRole("link", { name: /Monthly expenses/ }))
      .toHaveAttribute(
        "href",
        "/spaces/space-1/compositions/tool-1/revision-2",
      );
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() =>
      expect(compositionApi.list).toHaveBeenNthCalledWith(
        2,
        "space-1",
        50,
        50,
        expect.any(AbortSignal),
      )
    );
  });

  it("links a single new tool action to the studio", async () => {
    vi.mocked(compositionApi.list).mockResolvedValueOnce(emptyPage);
    render(() => <CompositionListRoute />);
    await screen.findByText("No saved tools");

    const actions = screen.getAllByRole("link", { name: "New tool" });
    expect(actions).toHaveLength(1);
    expect(actions[0]).toHaveAttribute(
      "href",
      "/spaces/space-1/compositions/new",
    );
  });

  it("offers a retry after the bounded list fails", async () => {
    vi.mocked(compositionApi.list)
      .mockRejectedValueOnce(new Error("unavailable"))
      .mockResolvedValueOnce(emptyPage);
    render(() => <CompositionListRoute />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not load saved tools",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(compositionApi.list).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("No saved tools")).toBeInTheDocument();
  });
});
