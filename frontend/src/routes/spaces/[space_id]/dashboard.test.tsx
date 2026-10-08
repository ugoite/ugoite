import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { createMemo, createSignal, type JSX, Show } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { compositionApi } from "~/lib/composition-api";
import { formApi, protocolFetch } from "~/lib/ugoite-client";
import SpaceDashboardRoute from "./dashboard";

const navigate = vi.fn();
const { entryStoreMock } = vi.hoisted(() => ({
  entryStoreMock: { entries: vi.fn(), loadEntries: vi.fn(), error: vi.fn() },
}));
vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigate,
  useParams: () => ({ space_id: "default" }),
  A: (props: { href: string; class?: string; children: JSX.Element }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
}));
vi.mock(
  "~/components/create-dialogs",
  () => ({
    CreateFormDialog: (props: { open: boolean }) => {
      const open = createMemo(() => props.open);
      return (
        <Show when={open()}>
          <div>Create Form Dialog</div>
        </Show>
      );
    },
  }),
);
vi.mock(
  "~/lib/entry-store",
  () => ({ createEntryStore: () => entryStoreMock }),
);
vi.mock(
  "~/lib/ugoite-client",
  () => ({
    formApi: { list: vi.fn(), listTypes: vi.fn(), create: vi.fn() },
    protocolFetch: vi.fn(),
  }),
);

describe("v5 space Home", () => {
  beforeEach(() => {
    navigate.mockReset();
    setLocale("en");
    entryStoreMock.entries.mockReturnValue([]);
    entryStoreMock.loadEntries.mockResolvedValue(undefined);
    entryStoreMock.error.mockReturnValue(null);
    vi.mocked(formApi.listTypes).mockResolvedValue([]);
    vi.mocked(protocolFetch).mockResolvedValue({
      items: [],
      offset: 0,
      limit: 3,
      has_more: false,
    } as never);
  });
  it("renders Continue, Pinned and Recent without metric cards", async () => {
    vi.mocked(formApi.list).mockResolvedValue([{
      name: "Notes",
      version: 1,
      template: "",
      fields: { body: { type: "markdown", required: false } },
    }]);
    render(() => <SpaceDashboardRoute />);
    expect(await screen.findByRole("heading", { name: "Home" }))
      .toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Continue" }))
      .toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Pinned" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Recent" })).toBeInTheDocument();
    expect(screen.queryByText(/forms available/i)).not.toBeInTheDocument();
    expect(document.querySelector(".continueGrid")).toBeInTheDocument();
    expect(document.querySelector(".continueGrid .card")).toBeNull();
    expect(document.querySelector(".pinGrid")).toBeInTheDocument();
  });
  it("REQ-UX-LIST-001: gives same-Form Recent links unique ID-free labels", async () => {
    entryStoreMock.entries.mockReturnValue([
      {
        id: "entry-old-no-title",
        form: "Notes",
        updated_at: "2026-10-05T00:00:00Z",
        properties: {},
        tags: [],
      },
      {
        id: "entry-plan",
        form: "Notes",
        updated_at: "2026-10-06T00:00:00Z",
        properties: { title: "Quarterly plan" },
        tags: [],
      },
      {
        id: "entry-travel",
        form: "Notes",
        updated_at: "2026-10-07T00:00:00Z",
        properties: {
          name: "Travel notes",
          private_note: "dashboard-recent-unrelated-field-marker",
        },
        tags: [],
      },
      {
        id: "entry-latest-no-title",
        form: "Notes",
        updated_at: "2026-10-08T00:00:00Z",
        properties: {},
        tags: [],
      },
    ]);
    vi.mocked(formApi.list).mockResolvedValue([]);

    render(() => <SpaceDashboardRoute />);

    const recentSection = screen.getByRole("heading", { name: "Recent" })
      .closest<HTMLElement>("section");
    expect(recentSection).not.toBeNull();
    const recent = within(recentSection!);
    const recentLinks = recent.getAllByRole("link");
    expect(recentLinks).toHaveLength(3);

    const fallbackLink = recent.getByRole("link", { name: /Entry.*Notes/ });
    expect(fallbackLink).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-latest-no-title",
    );
    const planLink = recent.getByRole("link", {
      name: /Quarterly plan.*Notes/,
    });
    expect(planLink).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-plan",
    );
    const travelLink = recent.getByRole("link", {
      name: /Travel notes.*Notes/,
    });
    expect(travelLink).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-travel",
    );

    for (const link of recentLinks) {
      expect(link).not.toHaveAccessibleName(
        /entry-(?:old-no-title|plan|travel|latest-no-title)/,
      );
      expect(link).not.toHaveAccessibleName(
        /dashboard-recent-unrelated-field-marker/,
      );
    }
    expect(document.body.textContent).not.toMatch(
      /entry-(?:old-no-title|plan|travel|latest-no-title)/,
    );
    expect(document.body.textContent).not.toContain(
      "dashboard-recent-unrelated-field-marker",
    );
    expect(
      screen.queryByRole("link", { name: /entry-old-no-title/i }),
    ).not.toBeInTheDocument();
  });
  it("rediscovers saved tools from Home and opens the listed exact revision", async () => {
    vi.mocked(formApi.list).mockResolvedValue([]);
    vi.mocked(protocolFetch).mockResolvedValue({
      items: [{
        composition_id: "composition-1",
        revision_id: "revision-7",
        updated_at: 1772960822,
        name: "Monthly expenses",
        kind: "dashboard",
        format_version: 1,
        tags: [],
      }],
      offset: 0,
      limit: 3,
      has_more: false,
    } as never);
    render(() => <SpaceDashboardRoute />);

    const row = await screen.findByRole("link", { name: /Monthly expenses/ });
    expect(row).toHaveAttribute(
      "href",
      "/spaces/default/compositions/composition-1/revision-7",
    );
    expect(screen.getByRole("link", { name: "All" })).toHaveAttribute(
      "href",
      "/spaces/default/compositions",
    );
    expect(document.querySelector(".rowList .card")).toBeNull();
  });
  it("links saved tools to the new tool studio action", async () => {
    vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);

    await screen.findByRole("heading", { name: "Saved tools" });
    expect(screen.getByRole("link", { name: "New tool" })).toHaveAttribute(
      "href",
      "/spaces/default/compositions/new",
    );
  });

  it("starts the dedicated New Entry route when a creatable Form exists", async () => {
    vi.mocked(formApi.list).mockResolvedValue([{
      name: "Notes",
      version: 1,
      template: "",
      fields: {},
    }]);
    render(() => <SpaceDashboardRoute />);
    const button = (await screen.findAllByRole("button", { name: /Entry/ }))[0];
    fireEvent.click(button);
    expect(navigate).toHaveBeenCalledWith("/spaces/default/entries/new");
  });
  it("opens Form creation and shows the walkthrough for a fresh Space", async () => {
    vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);
    fireEvent.click(
      (await screen.findAllByRole("button", { name: /Entry/ }))[0],
    );
    expect(screen.getByText("Create Form Dialog")).toBeInTheDocument();
    expect(
      await screen.findByRole("link", {
        name: "Create your first entry with the browser walkthrough",
      }),
    )
      .toHaveAttribute(
        "href",
        "https://ugoite.github.io/ugoite/docs/get-started/quickstart",
      );
  });
  it("shows a loading state and keeps entry creation disabled until forms are ready", async () => {
    let resolveForms: (
      forms: Array<
        {
          name: string;
          version: number;
          template: string;
          fields: Record<string, never>;
        }
      >,
    ) => void;
    vi.mocked(formApi.list).mockReturnValue(
      new Promise((resolve) => {
        resolveForms = resolve;
      }),
    );
    render(() => <SpaceDashboardRoute />);
    expect(screen.getAllByRole("status")[0]).toHaveTextContent(
      "Loading forms...",
    );
    expect(screen.getAllByRole("button", { name: /Entry/ })[0]).toBeDisabled();

    resolveForms!([{ name: "Notes", version: 1, template: "", fields: {} }]);
    await waitFor(() => {
      expect(screen.getAllByRole("button", { name: /Entry/ })[0]).toBeEnabled();
    });
    await waitFor(() => {
      expect(
        screen.queryAllByRole("status").every((status) =>
          !status.textContent?.includes("Loading forms...")
        ),
      ).toBe(true);
    });
  });
  it("keeps entry creation disabled after a form load failure and offers retry", async () => {
    let rejectForms: (reason?: unknown) => void;
    vi.mocked(formApi.list).mockReturnValue(
      new Promise((_, reject) => {
        rejectForms = reject;
      }),
    );
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

    expect(await screen.findByText("Start by creating your first form."))
      .toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create your first form" }))
      .toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /Entry/ })[0]).toBeEnabled();
  });
  it("does not show walkthrough guidance while existing entries are loading", async () => {
    const [mockEntries, setMockEntries] = createSignal<
      Array<
        {
          id: string;
          title: string;
          form: string;
          updated_at: string;
          properties: Record<string, never>;
          tags: never[];
        }
      >
    >([]);
    let resolveLoad: () => void;
    entryStoreMock.entries.mockImplementation(mockEntries);
    entryStoreMock.loadEntries.mockReturnValue(
      new Promise<void>((resolve) => {
        resolveLoad = resolve;
      }),
    );
    vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);
    expect(screen.queryByRole("link", { name: /browser walkthrough/ })).not
      .toBeInTheDocument();

    setMockEntries([{
      id: "entry-1",
      title: "API memo",
      form: "Notes",
      updated_at: "2026-01-01",
      properties: {},
      tags: [],
    }]);
    resolveLoad!();
    await waitFor(() => expect(entryStoreMock.loadEntries).toHaveBeenCalled());
    expect(screen.queryByRole("link", { name: /browser walkthrough/ })).not
      .toBeInTheDocument();
  });
  it("does not show walkthrough guidance when the entry load fails", async () => {
    entryStoreMock.error.mockReturnValue("Failed to load entries");
    render(() => <SpaceDashboardRoute />);
    await waitFor(() =>
      expect(screen.queryByRole("link", { name: /browser walkthrough/ })).not
        .toBeInTheDocument()
    );
  });
  it("uses the Japanese v5 copy", async () => {
    setLocale("ja");
    vi.mocked(formApi.list).mockResolvedValue([]);
    render(() => <SpaceDashboardRoute />);
    expect(await screen.findByRole("heading", { name: "ホーム" }))
      .toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "続きから" }))
      .toBeInTheDocument();
  });
});
