import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import SpaceSearchRoute from "./search";

const searchParams: Record<string, string | string[] | undefined> = {};
const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
  }) => (
    <a href={props.href} class={props.class} aria-label={props["aria-label"]}>
      {props.children}
    </a>
  ),
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default" }),
  useSearchParams: () => [searchParams, vi.fn()],
}));

const entryRow = (id: string, preview: string) => ({
  id,
  form_id: "form-1",
  revision_id: "rev-1",
  created_at_micros: 1_772_960_000_000_000,
  updated_at_micros: 1_772_963_000_000_000,
  preview,
});

describe("/spaces/:space_id/search", () => {
  beforeEach(() => {
    setLocale("en");
    navigateMock.mockReset();
    for (const key of Object.keys(searchParams)) delete searchParams[key];
    vi.restoreAllMocks();
    vi.spyOn(entryApi, "query").mockResolvedValue({
      rows: [],
      has_more: false,
    });
  });

  it("keeps typed text in the field while the committed query waits for submit", async () => {
    render(() => <SpaceSearchRoute />);
    const field = screen.getByRole("textbox", {
      name: "Search keywords",
    }) as HTMLInputElement;

    field.focus();
    fireEvent.input(field, { target: { value: "hello" } });
    expect(field.value).toBe("hello");
    expect(document.activeElement).toBe(field);
    // Typing alone does not commit or execute a query.
    expect(entryApi.query).not.toHaveBeenCalled();

    fireEvent.input(field, { target: { value: "hello world" } });
    expect(field.value).toBe("hello world");
    expect(entryApi.query).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));
    await waitFor(() => expect(entryApi.query).toHaveBeenCalledTimes(1));
    const request = vi.mocked(entryApi.query).mock.calls[0][1];
    expect(request.query.text).toBe("hello world");
    // The draft survives the committed execution.
    expect(
      (screen.getByRole("textbox", {
        name: "Search keywords",
      }) as HTMLInputElement)
        .value,
    ).toBe("hello world");
  });

  it("commits surrounding whitespace once at the submit boundary", async () => {
    render(() => <SpaceSearchRoute />);
    const field = screen.getByRole("textbox", { name: "Search keywords" });

    fireEvent.input(field, { target: { value: "  padded  " } });
    expect(entryApi.query).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));
    await waitFor(() => expect(entryApi.query).toHaveBeenCalledTimes(1));
    expect(vi.mocked(entryApi.query).mock.calls[0][1].query.text).toBe(
      "padded",
    );
  });

  it("uses a deep link as the field value and the first search condition", async () => {
    searchParams.q = "deep linked";
    render(() => <SpaceSearchRoute />);

    expect(
      (screen.getByRole("textbox", {
        name: "Search keywords",
      }) as HTMLInputElement)
        .value,
    ).toBe("deep linked");
    await waitFor(() => expect(entryApi.query).toHaveBeenCalledTimes(1));
    expect(vi.mocked(entryApi.query).mock.calls[0][1].query.text).toBe(
      "deep linked",
    );
  });

  it("opens the selected result as an Entry", async () => {
    vi.mocked(entryApi.query).mockResolvedValue({
      rows: [entryRow("entry-1", "Readable entry")],
      has_more: false,
    });
    render(() => <SpaceSearchRoute />);
    const field = screen.getByRole("textbox", { name: "Search keywords" });

    fireEvent.input(field, { target: { value: "readable" } });
    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));

    const cell = await screen.findByText("Readable entry");
    const rowButton = cell.closest("tr")?.querySelector("button");
    expect(rowButton).not.toBeNull();
    fireEvent.click(rowButton!);
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/default/entries/entry-1",
    );
  });
});
