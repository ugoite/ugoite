import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { createMemo, createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";
import SpaceEntriesIndexPane from "./index";

const searchParams: Record<string, string> = {};
const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigate,
  useSearchParams: () => [searchParams, vi.fn()],
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
        <SpaceEntriesIndexPane />
      </EntriesRouteContext.Provider>
    );
  });
}

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

describe("/spaces/:space_id/entries", () => {
  beforeEach(() => {
    setLocale("en");
    navigate.mockReset();
    for (const key of Object.keys(searchParams)) delete searchParams[key];
  });

  it("mounts the shared EntryBrowser for the canonical All Forms surface", () => {
    renderRoute([], "default", true);

    expect(screen.getByRole("heading", { name: "Entries" }))
      .toBeInTheDocument();
    expect(screen.getByRole("toolbar", { name: "Entry query" }))
      .toBeInTheDocument();
    expect(
      screen.getByRole("toolbar", { name: "Entry query" })
        .querySelectorAll("summary"),
    ).toHaveLength(3);
  });

  it("uses Form scope capabilities and keeps create beside the browser", () => {
    searchParams.form = "Notes";
    renderRoute([noteForm], "default", true);

    expect(screen.getByRole("heading", { name: "Notes" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Forms" }))
      .toHaveAttribute("href", "/spaces/default/forms");
    const toolbar = screen.getByRole("toolbar", { name: "Entry query" });
    expect(toolbar.textContent).toContain("title");
    const create = screen.getByRole("button", { name: "+ Entry" });
    expect(create).toBeInTheDocument();
    expect(document.querySelector(".entriesHeader")!.contains(create))
      .toBe(false);
  });

  it("does not query or render a browser for an unknown Form", () => {
    searchParams.form = "Missing";
    renderRoute([noteForm]);

    expect(screen.getByRole("heading", { name: "Missing" }))
      .toBeInTheDocument();
    expect(screen.getByText(/No such form “Missing”/)).toBeInTheDocument();
    expect(screen.queryByRole("toolbar", { name: "Entry query" }))
      .not.toBeInTheDocument();
  });
});
