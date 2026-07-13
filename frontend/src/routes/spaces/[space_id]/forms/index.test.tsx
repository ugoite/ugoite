import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";
import SpaceFormsIndexPane from "./index";

const search: Record<string,string> = {};
const setSearch = vi.fn();
vi.mock("@solidjs/router", () => ({ useNavigate: () => vi.fn(), useSearchParams: () => [search, setSearch] }));
vi.mock("~/components/SpaceShell", () => ({ SpaceShell: (props: { children: unknown }) => <div>{props.children}</div> }));
vi.mock("~/components/FormTable", () => ({ FormTable: (props: { entryForm: Form }) => <div>Entries table for {props.entryForm.name}</div> }));
vi.mock("~/components/create-dialogs", () => ({ CreateFormDialog: () => null }));
vi.mock("~/lib/ugoite-client", () => ({ assetApi: { list: vi.fn().mockResolvedValue([]) }, formApi: { create: vi.fn() } }));

const noteForm: Form = { name: "Notes", version: 1, template: "", fields: { title: { type: "string", required: true }, body: { type: "markdown", required: false } } };
function renderPage(forms: Form[]) {
  const [list] = createSignal(forms);
  render(() => <EntriesRouteContext.Provider value={{ spaceId: () => "default", forms: list, loadingForms: () => false, columnTypes: () => [], refetchForms: vi.fn(), entryStore: {} as never, spaceStore: {} as never }}><SpaceFormsIndexPane /></EntriesRouteContext.Provider>);
}
describe("v5 Forms workspace", () => {
  beforeEach(() => { setLocale("en"); setSearch.mockReset(); for (const key of Object.keys(search)) delete search[key]; });
  it("defaults to the first creatable Form", async () => {
    renderPage([noteForm]);
    await waitFor(() => expect(setSearch).toHaveBeenCalledWith({ form: "Notes", tab: "entries" }, { replace: true }));
  });
  it("renders split Form list, context and tabs", () => {
    search.form = "Notes"; search.tab = "entries"; renderPage([noteForm]);
    expect(screen.getByPlaceholderText("Find a Form")).toBeInTheDocument();
    expect(screen.getByText("Forms / Notes / Entries")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Entries" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Fields" })).toBeInTheDocument();
    expect(screen.getByText("Entries table for Notes")).toBeInTheDocument();
  });
  it("shows the v5 empty state and Japanese copy", () => {
    setLocale("ja"); renderPage([]);
    expect(screen.getByRole("heading", { name: "フォーム" })).toBeInTheDocument();
    expect(screen.getByText("フォームがありません")).toBeInTheDocument();
  });
});
