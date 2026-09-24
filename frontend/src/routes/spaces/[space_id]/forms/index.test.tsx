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

const refetchForms = vi.fn();
const navigate = vi.fn();
vi.mock(
  "@solidjs/router",
  () => ({
    useNavigate: () => navigate,
  }),
);
vi.mock(
  "~/components/SpaceShell",
  () => ({
    SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
  }),
);
vi.mock("~/components/create-dialogs", () => ({
  CreateFormDialog: (props: {
    open: boolean;
    onSubmit: (payload: { name: string }) => Promise<void>;
  }) => (
    <Show when={props.open}>
      <button
        type="button"
        onClick={() => void props.onSubmit({ name: "Projects" })}
      >
        Submit new form
      </button>
    </Show>
  ),
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
    formApi: { save: vi.fn() },
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
const spacedForm: Form = {
  ...noteForm,
  name: "My Form",
};
const metadataForm: Form = {
  name: "SQL",
  version: 1,
  template: "",
  fields: {},
};
function renderPage(forms: Form[], formsError?: unknown, spaceId = "default") {
  const [list] = createSignal(forms);
  render(() => (
    <EntriesRouteContext.Provider
      value={{
        spaceId: () => spaceId,
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
describe("Forms list", () => {
  beforeEach(() => {
    setLocale("en");
    navigate.mockReset();
    refetchForms.mockReset();
    vi.mocked(formApi.save).mockReset();
  });
  it("renders Forms and exposes the create action accessibly", () => {
    renderPage([noteForm]);
    expect(screen.getByRole("button", { name: "New Form" }))
      .toBeInTheDocument();
    expect(
      screen.getByRole("list", { name: "Forms" }),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
  });
  it("navigates to the form-scoped Entry list on row click", () => {
    renderPage([noteForm]);
    fireEvent.click(document.querySelector(".rowListMain")!);
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/forms/Notes/entries",
    );
  });
  it("encodes Form names in the entries navigation target", () => {
    renderPage([spacedForm]);
    fireEvent.click(document.querySelector(".rowListMain")!);
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/default/forms/My%20Form/entries",
    );
  });
  it("encodes Space path segments when navigating to the Entry list", () => {
    renderPage([noteForm], undefined, "space/with space");
    fireEvent.click(document.querySelector(".rowListMain")!);
    expect(navigate).toHaveBeenCalledWith(
      "/spaces/space%2Fwith%20space/forms/Notes/entries",
    );
  });
  it("passes the logical Space ID to the API after an encoded navigation", async () => {
    vi.mocked(formApi.save).mockResolvedValue(noteForm);
    renderPage([noteForm], undefined, "space/with space");
    fireEvent.click(screen.getByRole("button", { name: "New Form" }));
    fireEvent.click(screen.getByRole("button", { name: "Submit new form" }));
    await waitFor(() =>
      expect(formApi.save).toHaveBeenCalledWith("space/with space", {
        name: "Projects",
      })
    );
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith(
        "/spaces/space%2Fwith%20space/forms/Projects/entries",
      )
    );
  });
  it("shares one list DOM between mobile and desktop", () => {
    renderPage([noteForm, { ...noteForm, name: "Projects" }]);
    expect(document.querySelector(".mobileFormPicker")).toBeNull();
    expect(document.querySelector(".desktopFormPicker")).toBeNull();
    expect(document.querySelector("select")).toBeNull();
    expect(screen.getAllByRole("list")).toHaveLength(1);
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
  });
  it("filters the list with the shared search field", () => {
    renderPage([noteForm, { ...noteForm, name: "Projects" }]);
    fireEvent.input(screen.getByRole("searchbox", { name: "Find a Form" }), {
      target: { value: "Proj" },
    });
    expect(screen.queryByText("Notes")).not.toBeInTheDocument();
    expect(
      Array.from(document.querySelectorAll(".formRowName")).map((node) =>
        node.textContent
      ),
    ).toEqual(["Projects"]);
    fireEvent.input(screen.getByRole("searchbox", { name: "Find a Form" }), {
      target: { value: "missing" },
    });
    expect(screen.getByText("No Forms yet")).toBeInTheDocument();
  });
  it("keeps system Forms hidden until the visibility toggle is enabled", () => {
    renderPage([noteForm, metadataForm]);
    expect(screen.queryByText("SQL")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("checkbox", { name: "Show system forms" }),
    );
    expect(
      Array.from(document.querySelectorAll(".formRowName")).some((node) =>
        node.textContent === "SQL"
      ),
    ).toBe(true);
    expect(screen.getByLabelText("System form")).toBeInTheDocument();
  });
  it("REQ-UX-RESP-001: keeps icon-only row actions named with tooltips", () => {
    renderPage([noteForm]);
    const edit = screen.getByRole("button", { name: "Edit Notes" });
    // Icon-only secondary action: accessible name plus hover tooltip
    // (POL-UI-011) so the control never depends on vision alone.
    expect(edit).toHaveAttribute("title", "Edit Notes");
    expect(edit.getAttribute("aria-label")).toBe("Edit Notes");
  });
  it("keeps edit on a small per-row button that does not navigate", async () => {
    vi.mocked(formApi.save).mockResolvedValue(noteForm);
    renderPage([noteForm]);
    const row = document.querySelector(".rowListMain")!;
    expect(
      within(row).queryByRole("button", { name: /Edit/ }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Edit Notes" }));
    expect(navigate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Submit form edit" }));
    await waitFor(() => expect(formApi.save).toHaveBeenCalled());
    expect(refetchForms).toHaveBeenCalled();
  });
  it("creates a Form and navigates to its Entry list", async () => {
    vi.mocked(formApi.save).mockResolvedValue(noteForm);
    renderPage([noteForm]);
    fireEvent.click(screen.getByRole("button", { name: "New Form" }));
    fireEvent.click(screen.getByRole("button", { name: "Submit new form" }));
    await waitFor(() =>
      expect(formApi.save).toHaveBeenCalledWith("default", {
        name: "Projects",
      })
    );
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith(
        "/spaces/default/forms/Projects/entries",
      )
    );
  });
  it("shows the empty state and Japanese copy", () => {
    setLocale("ja");
    renderPage([]);
    expect(screen.getByRole("heading", { name: "フォーム" }))
      .toBeInTheDocument();
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
