import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal, Show } from "solid-js";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { setLocale } from "~/lib/i18n";
import { formApi } from "~/lib/ugoite-client";
import type { Form } from "~/lib/types";
import SpaceFormsIndexPane from "./index";

const search: Record<string, string> = {};
const setSearch = vi.fn();
const refetchForms = vi.fn();
vi.mock(
  "@solidjs/router",
  () => ({
    useNavigate: () => vi.fn(),
    useSearchParams: () => [search, setSearch],
  }),
);
vi.mock(
  "~/components/SpaceShell",
  () => ({
    SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
  }),
);
vi.mock(
  "~/components/FormTable",
  () => ({
    FormTable: (props: { entryForm: Form }) => (
      <div>Entries table for {props.entryForm.name}</div>
    ),
  }),
);
vi.mock("~/components/create-dialogs", () => ({
  CreateFormDialog: () => null,
  EditFormDialog: (props: {
    open: boolean;
    entryForm: Form;
    onSubmit: (payload: Form) => Promise<void>;
  }) => (
    <Show when={props.open}>
      <button
        type="button"
        onClick={() => void props.onSubmit(props.entryForm)}
      >
        Submit form edit
      </button>
    </Show>
  ),
}));
vi.mock(
  "~/lib/ugoite-client",
  () => ({
    assetApi: { list: vi.fn().mockResolvedValue([]) },
    formApi: { create: vi.fn() },
  }),
);

const noteForm: Form = {
  name: "Notes",
  version: 1,
  template: "",
  fields: {
    title: { type: "string", required: true },
    body: { type: "markdown", required: false },
  },
};
function renderPage(forms: Form[], formsError?: unknown) {
  const [list] = createSignal(forms);
  render(() => (
    <EntriesRouteContext.Provider
      value={{
        spaceId: () => "default",
        forms: list,
        loadingForms: () => false,
        formsError: () => formsError,
        columnTypes: () => [],
        refetchForms,
        entryStore: {} as never,
        spaceStore: {} as never,
      }}
    >
      <SpaceFormsIndexPane />
    </EntriesRouteContext.Provider>
  ));
}
describe("v5 Forms workspace", () => {
  beforeEach(() => {
    setLocale("en");
    setSearch.mockReset();
    refetchForms.mockReset();
    vi.mocked(formApi.create).mockReset();
    for (const key of Object.keys(search)) delete search[key];
  });
  it("defaults to the first creatable Form", async () => {
    renderPage([noteForm]);
    await waitFor(() =>
      expect(setSearch).toHaveBeenCalledWith(
        { form: "Notes", tab: undefined },
        { replace: true },
      )
    );
  });
  it("renders one Form workspace without duplicate view tabs", () => {
    search.form = "Notes";
    renderPage([noteForm]);
    expect(screen.getByPlaceholderText("Find a Form")).toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(screen.getAllByText("Notes")).toHaveLength(1);
    expect(screen.getByText("Entries table for Notes")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Form" }).querySelector("svg"))
      .toBeInTheDocument();
  });
  it("opens and submits the edit Form dialog", async () => {
    search.form = "Notes";
    vi.mocked(formApi.create).mockResolvedValue(noteForm);
    renderPage([noteForm]);

    fireEvent.click(screen.getByRole("button", { name: "Edit Form" }));
    fireEvent.click(screen.getByRole("button", { name: "Submit form edit" }));

    await waitFor(() => expect(formApi.create).toHaveBeenCalled());
    expect(refetchForms).toHaveBeenCalled();
  });
  it("shows the v5 empty state and Japanese copy", () => {
    setLocale("ja");
    renderPage([]);
    expect(screen.getByText("フォーム")).toBeInTheDocument();
    expect(screen.getByText("フォームがありません")).toBeInTheDocument();
  });
  it("shows API failures instead of an empty Forms state", () => {
    renderPage([], new Error("Forbidden"));
    expect(screen.getByText("Failed to load Forms.")).toBeInTheDocument();
    expect(screen.queryByText("No Forms yet")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(refetchForms).toHaveBeenCalled();
  });
});
