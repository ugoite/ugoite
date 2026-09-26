import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { entryApi, formApi } from "~/lib/ugoite-client";
import SpaceSearchRoute from "./search";

const searchParams: Record<string, string | string[] | undefined> = {};
const navigateMock = vi.fn();
let mockReadSpaceId = () => "default";

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
  useParams: () => ({
    get space_id() {
      return mockReadSpaceId();
    },
  }),
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
    mockReadSpaceId = () => "default";
    for (const key of Object.keys(searchParams)) delete searchParams[key];
    vi.restoreAllMocks();
    vi.spyOn(entryApi, "query").mockResolvedValue({
      rows: [],
      has_more: false,
    });
    vi.spyOn(formApi, "list").mockResolvedValue([]);
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

  it("resolves each Search result Form by form_id", async () => {
    vi.mocked(formApi.list).mockResolvedValue([
      { id: "form-1", name: "Tasks" } as never,
      { id: "form-2", name: "Notes" } as never,
    ]);
    vi.mocked(entryApi.query).mockResolvedValue({
      rows: [
        entryRow("entry-1", "Task preview"),
        { ...entryRow("entry-2", "Note preview"), form_id: "form-2" },
        {
          ...entryRow("entry-3", "Unresolved preview"),
          form_id: "form-missing",
        },
      ],
      has_more: false,
    });
    render(() => <SpaceSearchRoute />);

    fireEvent.input(screen.getByRole("textbox", { name: "Search keywords" }), {
      target: { value: "matching" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));

    const taskPreview = await screen.findByText("Task preview");
    const taskRow = taskPreview.closest("tr");
    const notePreview = screen.getByText("Note preview");
    const noteRow = notePreview.closest("tr");
    const unresolvedPreview = screen.getByText("Unresolved preview");
    const unresolvedRow = unresolvedPreview.closest("tr");
    expect(taskRow).not.toBeNull();
    expect(noteRow).not.toBeNull();
    expect(unresolvedRow).not.toBeNull();
    expect(taskRow).toHaveTextContent("Tasks");
    expect(noteRow).toHaveTextContent("Notes");
    expect(unresolvedRow).toHaveTextContent("Unknown form");
    expect(formApi.list).toHaveBeenCalledTimes(1);
    expect(formApi.list).toHaveBeenCalledWith("default");
  });

  it("keeps results visible while Form metadata loads and updates labels in place", async () => {
    let resolveForms!: (forms: never[]) => void;
    vi.mocked(formApi.list).mockReturnValue(
      new Promise((resolve) => {
        resolveForms = resolve;
      }),
    );
    vi.mocked(entryApi.query).mockResolvedValue({
      rows: [entryRow("entry-1", "Readable entry")],
      has_more: false,
    });
    render(() => <SpaceSearchRoute />);

    fireEvent.input(screen.getByRole("textbox", { name: "Search keywords" }), {
      target: { value: "readable" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));

    expect(await screen.findByText("Readable entry")).toBeInTheDocument();
    expect(screen.getByText("—")).toBeInTheDocument();
    expect(screen.getByText("Loading Form names…")).toBeInTheDocument();
    expect(entryApi.query).toHaveBeenCalledTimes(1);
    resolveForms([{ id: "form-1", name: "Tasks" } as never]);

    expect(await screen.findByText("Tasks")).toBeInTheDocument();
    expect(entryApi.query).toHaveBeenCalledTimes(1);
  });

  it("retries only Form metadata after a list failure", async () => {
    vi.mocked(formApi.list)
      .mockRejectedValueOnce(new Error("metadata unavailable"))
      .mockResolvedValueOnce([{ id: "form-1", name: "Tasks" } as never]);
    vi.mocked(entryApi.query).mockResolvedValue({
      rows: [entryRow("entry-1", "Readable entry")],
      has_more: false,
    });
    render(() => <SpaceSearchRoute />);

    fireEvent.input(screen.getByRole("textbox", { name: "Search keywords" }), {
      target: { value: "readable" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));

    expect(await screen.findByText("Readable entry")).toBeInTheDocument();
    expect(await screen.findByText("Form information is unavailable."))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByText("Tasks")).toBeInTheDocument();
    expect(screen.queryByText("Form information is unavailable.")).not
      .toBeInTheDocument();
    expect(formApi.list).toHaveBeenCalledTimes(2);
    expect(entryApi.query).toHaveBeenCalledTimes(1);
  });

  it("does not apply a late Form response from the previous Space", async () => {
    const [spaceId, setSpaceId] = createSignal("space-a");
    mockReadSpaceId = spaceId;
    let resolveA!: (forms: never[]) => void;
    let resolveB!: (forms: never[]) => void;
    vi.mocked(formApi.list).mockImplementation((requestedSpaceId) =>
      new Promise((resolve) => {
        if (requestedSpaceId === "space-a") resolveA = resolve;
        else resolveB = resolve;
      })
    );
    vi.mocked(entryApi.query).mockResolvedValue({
      rows: [entryRow("entry-1", "Readable entry")],
      has_more: false,
    });
    render(() => <SpaceSearchRoute />);

    fireEvent.input(screen.getByRole("textbox", { name: "Search keywords" }), {
      target: { value: "readable" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search entries" }));
    expect(await screen.findByText("Readable entry")).toBeInTheDocument();
    expect(screen.getByText("—")).toBeInTheDocument();

    setSpaceId("space-b");
    await waitFor(() => expect(formApi.list).toHaveBeenCalledWith("space-b"));
    resolveA([{ id: "form-1", name: "Space A Form" } as never]);
    expect(screen.getByText("—")).toBeInTheDocument();

    resolveB([{ id: "form-1", name: "Space B Form" } as never]);
    expect(await screen.findByText("Space B Form")).toBeInTheDocument();
    expect(screen.queryByText("Space A Form")).not.toBeInTheDocument();
  });
});
