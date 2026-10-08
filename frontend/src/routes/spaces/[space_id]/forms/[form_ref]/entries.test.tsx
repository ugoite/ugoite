import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
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
    const toolbar = screen.getByRole("toolbar", { name: "Entry browser" });
    expect(within(toolbar).getAllByRole("searchbox")).toHaveLength(1);
    expect(
      within(toolbar).queryByRole("link", { name: "Open saved queries" }),
    ).not.toBeInTheDocument();
    fireEvent.click(within(toolbar).getByRole("button", { name: "Columns" }));
    expect(screen.getByRole("checkbox", { name: "title" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "New Entry" }))
      .toBeInTheDocument();
  });

  it("opens Entry creation for the current Form", () => {
    renderRoute([noteForm]);

    fireEvent.click(screen.getByRole("button", { name: "New Entry" }));
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/entries/new?form=Notes",
    );
  });

  it("passes the selected Form ID scope into the Entry query request", async () => {
    renderRoute([noteForm]);

    await waitFor(() => expect(vi.mocked(entryApi.query)).toHaveBeenCalled());
    await screen.findByRole("toolbar", { name: "Entry browser" });
    const calls = vi.mocked(entryApi.query).mock.calls;
    expect(calls.length).toBeGreaterThan(0);
    for (const call of calls) {
      expect(call[0]).toBe("default");
      expect(call[1].query.scope).toEqual({
        kind: "form",
        form_id: noteForm.id,
      });
    }
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

  it("opens creation with the current Form selected", () => {
    renderRoute([noteForm]);

    fireEvent.click(screen.getByRole("button", { name: "New Entry" }));
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/entries/new?form=Notes",
    );
  });

  it("REQ-UX-LIST-001: keeps Entry IDs out of form rows with compact right-meta dates", async () => {
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

    const row = await screen.findByRole("row", { name: /Hello/ });
    expect(row).not.toHaveAccessibleName(/entry-1/);
    expect(document.body.textContent).not.toContain("entry-1");
    expect(container.querySelector('[class*="chip"]')).toBeNull();
    expect(
      screen.getAllByText(
        formatDateLabel(new Date(UPDATED_MICROS / 1_000).toISOString()),
      ).length,
    ).toBeGreaterThan(0);
  });

  it("navigates to the studio with the entry query seed", async () => {
    renderRoute([noteForm]);

    const saveButton = await screen.findByRole("button", {
      name: "Save as tool",
    });
    expect(saveButton).toBeEnabled();
    fireEvent.click(saveButton);

    expect(navigate).toHaveBeenCalledTimes(1);
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/compositions/new",
      {
        state: {
          seed: {
            kind: "entry_query",
            seed: {
              formId: noteForm.id,
              name: "Notes",
              fieldSchema: [{ field_id: 7, field_type: "string" }],
              query: {
                filters: [],
                sort: [],
                projection: { kind: "fields", fields: [7] },
              },
            },
          },
        },
      },
    );
    expect(screen.queryByRole("dialog", { name: "Save as tool" }))
      .not.toBeInTheDocument();
  });
});
