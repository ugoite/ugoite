import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { Show } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import SpaceDashboardRoute from "./dashboard";

const navigate = vi.fn();
vi.mock("@solidjs/router", () => ({ useNavigate: () => navigate, useParams: () => ({ space_id: "default" }), A: (props: { href: string; class?: string; children: unknown }) => <a href={props.href} class={props.class}>{props.children}</a> }));
vi.mock("~/components/SpaceShell", () => ({ SpaceShell: (props: { children: unknown }) => <div>{props.children}</div> }));
vi.mock("~/components/create-dialogs", () => ({ CreateFormDialog: (props: { open: boolean }) => <Show when={props.open}><div>Create Form Dialog</div></Show> }));
vi.mock("~/lib/entry-store", () => ({ createEntryStore: () => ({ entries: () => [], loadEntries: vi.fn() }) }));
vi.mock("~/lib/ugoite-client", () => ({ formApi: { list: vi.fn(), listTypes: vi.fn(), create: vi.fn() }, spaceApi: { get: vi.fn() } }));

describe("v5 space Home", () => {
  beforeEach(() => { navigate.mockReset(); setLocale("en"); vi.mocked(spaceApi.get).mockResolvedValue({ id: "default", name: "Local Knowledge", created_at: "2026-01-01" }); vi.mocked(formApi.listTypes).mockResolvedValue([]); });
  it("renders Continue, Pinned and Recent without metric cards", async () => {
    vi.mocked(formApi.list).mockResolvedValue([{ name: "Notes", version: 1, template: "", fields: { body: { type: "markdown", required: false } } }]);
    render(() => <SpaceDashboardRoute />);
    expect(await screen.findByRole("heading", { name: "Home" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Continue" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Pinned" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Recent" })).toBeInTheDocument();
    expect(screen.queryByText(/forms available/i)).not.toBeInTheDocument();
  });
  it("starts the dedicated New Entry route when a creatable Form exists", async () => {
    vi.mocked(formApi.list).mockResolvedValue([{ name: "Notes", version: 1, template: "", fields: {} }]);
    render(() => <SpaceDashboardRoute />);
    const button = (await screen.findAllByRole("button", { name: /Entry/ }))[0];
    fireEvent.click(button);
    expect(navigate).toHaveBeenCalledWith("/spaces/default/entries/new");
  });
  it("opens Form creation when the Space has no creatable Forms", async () => {
    vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: /Entry/ }))[0]);
    expect(screen.getByText("Create Form Dialog")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Read the browser-first walkthrough" }))
      .toHaveAttribute(
        "href",
        "https://ugoite.github.io/ugoite/docs/guide/browser-first-entry",
      );
  });
  it("uses the Japanese v5 copy", async () => {
    setLocale("ja"); vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);
    expect(await screen.findByRole("heading", { name: "ホーム" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "続きから" })).toBeInTheDocument();
  });
});
