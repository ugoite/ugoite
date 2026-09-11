import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
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
const navigate = vi.fn();
vi.mock(
  "@solidjs/router",
  () => ({
    useNavigate: () => navigate,
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
    FormTable: (props: { entryForm: Form; onAddRow?: () => void }) => (
      <div>
        <div>Entries table for {props.entryForm.name}</div>
        <Show when={props.onAddRow}>
          <button type="button" onClick={() => props.onAddRow?.()}>
            Add Row
          </button>
        </Show>
      </div>
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
const metadataForm: Form = {
  name: "SQL",
  version: 1,
  template: "",
  fields: {},
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
    navigate.mockReset();
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
    expect(document.querySelectorAll(".desktopFormPicker .formItem b"))
      .toHaveLength(1);
    expect(screen.getByRole("option", { name: "Notes" })).toBeInTheDocument();
    expect(screen.getByText("Entries table for Notes")).toBeInTheDocument();
    expect(
      within(document.querySelector(".desktopFormPicker")!).getByRole(
        "button",
        { name: "Form" },
      ).querySelector("svg"),
    ).toBeInTheDocument();
  });
  it("opens the canonical editor with the selected Form from Add Row", () => {
    search.form = "Notes";
    renderPage([noteForm]);

    fireEvent.click(screen.getByRole("button", { name: "Add Row" }));

    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/entries/new?form=Notes&returnTo=forms",
    );
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
  it("keeps system Forms hidden until the visibility toggle is enabled", () => {
    search.form = "Notes";
    renderPage([noteForm, metadataForm]);

    expect(screen.queryByText("SQL")).not.toBeInTheDocument();
    fireEvent.click(
      within(document.querySelector(".desktopFormPicker")!).getByRole(
        "checkbox",
        { name: "Show system forms" },
      ),
    );
    expect(
      Array.from(document.querySelectorAll(".desktopFormPicker .formItem b"))
        .some((node) => node.textContent === "SQL"),
    )
      .toBe(true);
    expect(screen.getByRole("option", { name: "SQL" })).toBeInTheDocument();
    expect(screen.getByLabelText("System form")).toBeInTheDocument();
    expect(screen.queryByText("System")).not.toBeInTheDocument();
  });
  it("does not offer entry creation for system Forms", () => {
    search.form = "SQL";
    renderPage([metadataForm]);

    fireEvent.click(
      within(document.querySelector(".desktopFormPicker")!).getByRole(
        "checkbox",
        { name: "Show system forms" },
      ),
    );

    expect(screen.getByText("Entries table for SQL")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add Row" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New Entry" })).not
      .toBeInTheDocument();
  });
  it("shows API failures instead of an empty Forms state", () => {
    renderPage([], new Error("Forbidden"));
    expect(screen.getByText("Failed to load Forms.")).toBeInTheDocument();
    expect(screen.queryByText("No Forms yet")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(refetchForms).toHaveBeenCalled();
  });
  it("provides a compact mobile Form picker without changing the route contract", () => {
    search.form = "Notes";
    renderPage([noteForm, { ...noteForm, name: "Projects" }]);

    const picker = screen.getByRole("combobox", { name: "Select a Form" });
    expect(picker).toHaveValue("Notes");

    fireEvent.change(picker, { target: { value: "Projects" } });

    expect(setSearch).toHaveBeenCalledWith({
      form: "Projects",
      tab: undefined,
    });
  });
  it("keeps system Form visibility and creation available in the mobile picker", () => {
    search.form = "Notes";
    renderPage([noteForm, metadataForm]);

    const picker = within(document.querySelector(".mobileFormPicker")!);
    expect(picker.getByRole("button", { name: "Form" })).toBeInTheDocument();
    fireEvent.click(picker.getByRole("checkbox", {
      name: "Show system forms",
    }));
    expect(picker.getByRole("option", { name: "SQL" })).toBeInTheDocument();
  });
});
