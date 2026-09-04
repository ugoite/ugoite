import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createMemo, createSignal, Show } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import SpaceDashboardRoute from "./dashboard";

const navigate = vi.fn();
const { entryStoreMock } = vi.hoisted(() => ({ entryStoreMock: { entries: vi.fn(), loadEntries: vi.fn(), error: vi.fn() } }));
vi.mock("@solidjs/router", () => ({ useNavigate: () => navigate, useParams: () => ({ space_id: "default" }), A: (props: { href: string; class?: string; children: unknown }) => <a href={props.href} class={props.class}>{props.children}</a> }));
vi.mock("~/components/create-dialogs", () => ({ CreateFormDialog: (props: { open: boolean }) => { const open = createMemo(() => props.open); return <Show when={open()}><div>Create Form Dialog</div></Show>; } }));
vi.mock("~/lib/entry-store", () => ({ createEntryStore: () => entryStoreMock }));
vi.mock("~/lib/ugoite-client", () => ({ formApi: { list: vi.fn(), listTypes: vi.fn(), create: vi.fn() }, spaceApi: { get: vi.fn() } }));

describe("v5 space Home", () => {
  beforeEach(() => { navigate.mockReset(); setLocale("en"); entryStoreMock.entries.mockReturnValue([]); entryStoreMock.loadEntries.mockResolvedValue(undefined); entryStoreMock.error.mockReturnValue(null); vi.mocked(spaceApi.get).mockResolvedValue({ id: "default", name: "Local Knowledge", created_at: "2026-01-01" }); vi.mocked(formApi.listTypes).mockResolvedValue([]); });
  it("renders Continue, Pinned and Recent without metric cards", async () => {
    vi.mocked(formApi.list).mockResolvedValue([{ name: "Notes", version: 1, template: "", fields: { body: { type: "markdown", required: false } } }]);
    render(() => <SpaceDashboardRoute />);
    expect(await screen.findByRole("heading", { name: "Home" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Continue" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Pinned" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Recent" })).toBeInTheDocument();
    expect(screen.getByText("Forms / Entries")).toBeInTheDocument();
    expect(screen.queryByText(/forms available/i)).not.toBeInTheDocument();
  });
  it("starts the dedicated New Entry route when a creatable Form exists", async () => {
    vi.mocked(formApi.list).mockResolvedValue([{ name: "Notes", version: 1, template: "", fields: {} }]);
    render(() => <SpaceDashboardRoute />);
    const button = (await screen.findAllByRole("button", { name: /Entry/ }))[0];
    fireEvent.click(button);
    expect(navigate).toHaveBeenCalledWith("/spaces/default/entries/new");
  });
  it("opens Form creation and shows the walkthrough for a fresh Space", async () => {
    vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: /Entry/ }))[0]);
    expect(screen.getByText("Create Form Dialog")).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "Create your first entry with the browser walkthrough" }))
      .toHaveAttribute("href", "https://ugoite.github.io/ugoite/docs/guide/start/browser-first-entry");
  });
  it("shows a loading state and keeps entry creation disabled until forms are ready", async () => {
    let resolveForms: (forms: Array<{ name: string; version: number; template: string; fields: Record<string, never> }>) => void;
    vi.mocked(formApi.list).mockReturnValue(new Promise((resolve) => {
      resolveForms = resolve;
    }));
    render(() => <SpaceDashboardRoute />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading forms...");
    expect(screen.getAllByRole("button", { name: /Entry/ })[0]).toBeDisabled();

    resolveForms!([{ name: "Notes", version: 1, template: "", fields: {} }]);
    await waitFor(() => {
      expect(screen.getAllByRole("button", { name: /Entry/ })[0]).toBeEnabled();
    });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
  it("keeps entry creation disabled after a form load failure and offers retry", async () => {
    let rejectForms: (reason?: unknown) => void;
    vi.mocked(formApi.list).mockReturnValue(new Promise((_, reject) => {
      rejectForms = reject;
    }));
    render(() => <SpaceDashboardRoute />);
    rejectForms!(new Error("forms unavailable"));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not load Forms",
    );
    expect(screen.getAllByRole("button", { name: /Entry/ })[0]).toBeDisabled();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
  it("distinguishes an empty form list from a failed form load", async () => {
    vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);

    expect(await screen.findByText("Start by creating your first form.")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create your first form" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /Entry/ })[0]).toBeEnabled();
  });
  it("does not show walkthrough guidance while existing entries are loading", async () => {
    const [mockEntries, setMockEntries] = createSignal<Array<{ id: string; title: string; form: string; updated_at: string; properties: Record<string, never>; tags: never[] }>>([]);
    let resolveLoad: () => void;
    entryStoreMock.entries.mockImplementation(mockEntries);
    entryStoreMock.loadEntries.mockReturnValue(new Promise<void>((resolve) => { resolveLoad = resolve; }));
    vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);
    expect(screen.queryByRole("link", { name: /browser walkthrough/ })).not.toBeInTheDocument();

    setMockEntries([{ id: "entry-1", title: "API memo", form: "Notes", updated_at: "2026-01-01", properties: {}, tags: [] }]);
    resolveLoad!();
    await waitFor(() => expect(entryStoreMock.loadEntries).toHaveBeenCalled());
    expect(screen.queryByRole("link", { name: /browser walkthrough/ })).not.toBeInTheDocument();
  });
  it("does not show walkthrough guidance when the entry load fails", async () => {
    entryStoreMock.error.mockReturnValue("Failed to load entries");
    render(() => <SpaceDashboardRoute />);
    await waitFor(() => expect(screen.queryByRole("link", { name: /browser walkthrough/ })).not.toBeInTheDocument());
  });
  it("uses the Japanese v5 copy", async () => {
    setLocale("ja"); vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);
    expect(await screen.findByRole("heading", { name: "ホーム" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "続きから" })).toBeInTheDocument();
  });
});
