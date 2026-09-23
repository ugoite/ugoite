import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { createMemo, createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";
import { entryApi } from "~/lib/ugoite-client";
import { formatDateLabel } from "~/lib/date-format";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";
import SpaceFormEntriesPane from "./entries";

const params: Record<string, string> = { form_ref: "Notes" };
const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigate,
  useParams: () => ({ space_id: "default", ...params }),
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
}));

function renderRoute(
  formsList: Form[] = [],
  spaceId = "default",
  loadingForms = false,
) {
  render(() => {
    const [forms] = createSignal(formsList);
    return (
      <EntriesRouteContext.Provider
        value={{
          spaceId: () => spaceId,
          forms: createMemo(forms),
          loadingForms: () => loadingForms,
          columnTypes: () => [],
          refetchForms: vi.fn(),
          entryStore: {} as ReturnType<typeof createEntryStore>,
          spaceStore: {} as ReturnType<typeof createSpaceStore>,
        }}
      >
        <SpaceFormEntriesPane />
      </EntriesRouteContext.Provider>
    );
  });
}

const CREATED_MICROS = 1_772_960_000_000_000;
const UPDATED_MICROS = 1_772_963_000_000_000;

const noteForm: Form = {
  id: "00000000-0000-7000-8000-000000000001",
  name: "Notes",
  version: 1,
  template: "",
  fields: {
    title: {
      id: 7,
      type: "string",
      required: true,
      query_capability: {
        field: { kind: "property", field_id: 7 },
        name: "title",
        field_type: "string",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals", "contains"],
      },
    },
  },
};

describe("/spaces/:space_id/forms/:form_ref/entries", () => {
  beforeEach(() => {
    setLocale("en");
    navigate.mockReset();
    params.form_ref = "Notes";
    vi.restoreAllMocks();
    vi.spyOn(entryApi, "query").mockResolvedValue({
      rows: [],
      has_more: false,
    });
  });

  it("uses Form scope capabilities and keeps create beside the browser", () => {
    renderRoute([noteForm], "default", true);

    expect(screen.getByRole("heading", { name: "Notes" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Forms" }))
      .toHaveAttribute("href", "/spaces/default/forms");
    const toolbar = screen.getByRole("toolbar", { name: "Entry browser" });
    expect(toolbar.textContent).toContain("title");
    const create = screen.getByRole("button", { name: "+ Entry" });
    expect(create).toBeInTheDocument();
    expect(document.querySelector(".entriesHeader")!.contains(create))
      .toBe(false);
  });

  it("opens Entry creation for the current Form", () => {
    renderRoute([noteForm]);

    fireEvent.click(screen.getByRole("button", { name: "+ Entry" }));
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/entries/new?form=Notes",
    );
  });

  it("selects a row to open its Entry", async () => {
    vi.mocked(entryApi.query).mockResolvedValue({
      rows: [{
        id: "entry-1",
        form_id: noteForm.id!,
        revision_id: "rev-1",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
        properties: { title: "Hello" },
        preview: "Hello",
      }],
      has_more: false,
    });
    renderRoute([noteForm]);

    const cell = await screen.findByText("Hello");
    const rowButton = cell.closest("tr")?.querySelector("button");
    expect(rowButton).not.toBeNull();
    fireEvent.click(rowButton!);
    expect(navigate).toHaveBeenCalledWith("/spaces/default/entries/entry-1");
  });

  it("does not query or render a browser for an unknown Form", () => {
    params.form_ref = "Missing";
    renderRoute([noteForm]);

    expect(screen.getByRole("heading", { name: "Missing" }))
      .toBeInTheDocument();
    expect(screen.getByText(/No such form “Missing”/)).toBeInTheDocument();
    expect(screen.queryByRole("toolbar", { name: "Entry browser" }))
      .not.toBeInTheDocument();
  });

  it("REQ-UX-ENTRY-001: renders form-scoped entries with a positional back link and list-adjacent create action", () => {
    renderRoute([noteForm]);

    const header = document.querySelector(".entriesHeader")!;
    const backLink = screen.getByRole("link", { name: "Back to Forms" });
    expect(header.contains(backLink)).toBe(true);
    const body = document.querySelector(".entriesBody")!;
    const create = screen.getByRole("button", { name: "+ Entry" });
    expect(body.contains(create)).toBe(true);
    expect(
      body.contains(screen.getByRole("toolbar", { name: "Entry browser" })),
    ).toBe(true);
  });

  it("REQ-UX-LIST-001: renders entry rows without type chips and with compact right-meta dates", async () => {
    vi.mocked(entryApi.query).mockResolvedValue({
      rows: [{
        id: "entry-1",
        form_id: noteForm.id!,
        revision_id: "rev-1",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
        properties: { title: "Hello" },
        preview: "Hello",
      }],
      has_more: false,
    });
    const { container } = render(() => {
      const [forms] = createSignal([noteForm]);
      return (
        <EntriesRouteContext.Provider
          value={{
            spaceId: () => "default",
            forms: createMemo(forms),
            loadingForms: () => false,
            columnTypes: () => [],
            refetchForms: vi.fn(),
            entryStore: {} as ReturnType<typeof createEntryStore>,
            spaceStore: {} as ReturnType<typeof createSpaceStore>,
          }}
        >
          <SpaceFormEntriesPane />
        </EntriesRouteContext.Provider>
      );
    });

    await screen.findByText("Hello");
    expect(container.querySelector('[class*="chip"]')).toBeNull();
    expect(
      screen.getAllByText(
        formatDateLabel(new Date(UPDATED_MICROS / 1_000).toISOString()),
      ).length,
    ).toBeGreaterThan(0);
  });
});
