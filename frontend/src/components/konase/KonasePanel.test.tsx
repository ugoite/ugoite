import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { KonaseWorkFailure } from "~/lib/konase/host";
import type { SelectedContextPreview } from "~/lib/konase/host";
import { KonasePanel } from "./KonasePanel";
import type { WritePreview } from "~/lib/konase/host";
import type { BrowserMcpAuthorizationOptions } from "~/lib/konase/browser-mcp-auth";

type FakeTurn = {
  outcome: { job_id: string; summary: string; meaningful: boolean };
  workId: string;
  undoAvailable: boolean;
  knowledge: "unchanged" | "saved" | "write_failed";
};

type FakeProgress =
  | { kind: "model" }
  | { kind: "mcp"; operation: string }
  | { kind: "complete"; summary: string }
  | { kind: "knowledge"; outcome: FakeTurn["knowledge"] }
  | { kind: "undo" };

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T | PromiseLike<T>) => void;
  reject: (reason?: unknown) => void;
};

type FakeKonaseHost = {
  submitDeferreds: Array<Deferred<FakeTurn>>;
  previewDeferreds: Array<Deferred<SelectedContextPreview>>;
  sendDeferreds: Array<Deferred<FakeTurn>>;
  selectedUriCalls: string[][];
  undoDeferreds: Array<Deferred<{ success: boolean }>>;
  listeners: Array<(progress: FakeProgress) => void>;
  pendingConfirmation?: WritePreview;
  cancelledConfirmations: string[];
  resolvedConfirmations: Array<{ requestId: string; approved: boolean }>;
  disposed: boolean;
  submit(prompt: string): Promise<FakeTurn>;
  previewSelectedContext(
    prompt: string,
    uris: string[],
  ): Promise<SelectedContextPreview>;
  sendSelectedContext(previewId: string): Promise<FakeTurn>;
  invalidateContextPreview(): void;
  undo(workId: string): Promise<{ success: boolean }>;
  resolveConfirmation(requestId: string, approved: boolean): boolean;
  cancelPending(): void;
  dispose(): void;
  requestConfirmation(preview: WritePreview): void;
  subscribeProgress(listener: (progress: FakeProgress) => void): () => void;
  emitProgress(progress: FakeProgress): void;
};

const {
  getSpaceMock,
  authorizeMock,
  listFormsMock,
  queryEntriesMock,
  hostInstances,
  createDeferred,
} = vi
  .hoisted(() => {
    const createDeferred = <T,>(): Deferred<T> => {
      let resolve!: Deferred<T>["resolve"];
      let reject!: Deferred<T>["reject"];
      const promise = new Promise<T>((resolvePromise, rejectPromise) => {
        resolve = resolvePromise;
        reject = rejectPromise;
      });
      return { promise, resolve, reject };
    };

    return {
      getSpaceMock: vi.fn(),
      authorizeMock: vi.fn(),
      listFormsMock: vi.fn(),
      queryEntriesMock: vi.fn(),
      hostInstances: [] as FakeKonaseHost[],
      createDeferred,
    };
  });

vi.mock("~/lib/konase/host", () => ({
  KonaseWriteDeniedError: class extends Error {
    readonly mutationOutcome = "rejected";
    readonly mutationCode = "MUTATION_REJECTED";
  },
  KonaseMutationUnconfirmedError: class extends Error {
    readonly mutationOutcome = "unknown";
    readonly mutationCode = "MUTATION_OUTCOME_UNKNOWN";
  },
  KonaseWorkFailure: class extends Error {
    constructor(
      readonly reason: unknown,
      readonly partial: {
        workId: string;
        jobId: string;
        knowledge: FakeTurn["knowledge"];
        undoAvailable: boolean;
      },
    ) {
      super(reason instanceof Error ? reason.message : "Work failed");
    }
  },
  KonaseHost: class {
    readonly submitDeferreds: Array<Deferred<FakeTurn>> = [];
    readonly previewDeferreds: Array<Deferred<SelectedContextPreview>> = [];
    readonly sendDeferreds: Array<Deferred<FakeTurn>> = [];
    readonly selectedUriCalls: string[][] = [];
    readonly undoDeferreds: Array<Deferred<{ success: boolean }>> = [];
    readonly listeners: Array<(progress: FakeProgress) => void> = [];
    pendingConfirmation?: WritePreview;
    readonly cancelledConfirmations: string[] = [];
    readonly resolvedConfirmations: Array<{
      requestId: string;
      approved: boolean;
    }> = [];
    disposed = false;
    private readonly onConfirmationRequired?: (preview: WritePreview) => void;
    private readonly onConfirmationCancelled?: (requestId: string) => void;

    constructor(options: {
      onConfirmationRequired?: (preview: WritePreview) => void;
      onConfirmationCancelled?: (requestId: string) => void;
    }) {
      this.onConfirmationRequired = options.onConfirmationRequired;
      this.onConfirmationCancelled = options.onConfirmationCancelled;
      hostInstances.push(this);
    }

    submit(_prompt: string): Promise<FakeTurn> {
      const deferred = createDeferred<FakeTurn>();
      this.submitDeferreds.push(deferred);
      return deferred.promise;
    }

    previewSelectedContext(
      _prompt: string,
      uris: string[],
    ): Promise<SelectedContextPreview> {
      this.selectedUriCalls.push([...uris]);
      const deferred = createDeferred<SelectedContextPreview>();
      this.previewDeferreds.push(deferred);
      return deferred.promise;
    }

    sendSelectedContext(_previewId: string): Promise<FakeTurn> {
      const deferred = createDeferred<FakeTurn>();
      this.sendDeferreds.push(deferred);
      return deferred.promise;
    }

    invalidateContextPreview(): void {}

    undo(_workId: string): Promise<{ success: boolean }> {
      const deferred = createDeferred<{ success: boolean }>();
      this.undoDeferreds.push(deferred);
      return deferred.promise;
    }

    resolveConfirmation(requestId: string, approved: boolean): boolean {
      if (this.pendingConfirmation?.requestId !== requestId) return false;
      this.pendingConfirmation = undefined;
      this.resolvedConfirmations.push({ requestId, approved });
      return true;
    }

    cancelPending(): void {
      const requestId = this.pendingConfirmation?.requestId;
      if (requestId) {
        this.pendingConfirmation = undefined;
        this.cancelledConfirmations.push(requestId);
        this.onConfirmationCancelled?.(requestId);
      }
    }

    dispose(): void {
      this.cancelPending();
      this.disposed = true;
    }

    requestConfirmation(preview: WritePreview): void {
      this.pendingConfirmation = preview;
      this.onConfirmationRequired?.(preview);
    }

    subscribeProgress(listener: (progress: FakeProgress) => void): () => void {
      this.listeners.push(listener);
      return () => undefined;
    }

    emitProgress(progress: FakeProgress): void {
      for (const listener of this.listeners) listener(progress);
    }
  },
}));
vi.mock("~/lib/konase/mcp", () => ({
  BrowserMcpHost: class {
    constructor(_options: unknown) {}
  },
}));
vi.mock("~/lib/konase/model", () => ({
  OpenAiModelHost: class {
    constructor(_options: unknown) {}
  },
}));

vi.mock("~/lib/ugoite-client", () => ({
  spaceApi: { get: getSpaceMock },
  formApi: { list: listFormsMock },
  entryApi: { query: queryEntriesMock },
}));
vi.mock("~/lib/konase/browser-mcp-auth", () => ({
  authorizeBrowserMcp: authorizeMock,
}));

const mockConnection = () => {
  getSpaceMock.mockImplementation(async (spaceId: string) => ({
    space_uid: `${spaceId}-uid`,
    name: spaceId,
    created_at: "",
  }));
  authorizeMock.mockImplementation(
    async (options: BrowserMcpAuthorizationOptions) => ({
      accessToken: `${options.spaceUid}-token`,
      endpoint: "/mcp",
      resource: `${location.origin}/mcp`,
      spaceUid: options.spaceUid,
    }),
  );
};

const fakeTurn = (summary: string): FakeTurn => ({
  outcome: { job_id: `job-${summary}`, summary, meaningful: true },
  workId: `work-${summary}`,
  undoAvailable: true,
  knowledge: "saved",
});

const fakeWritePreview = (
  overrides: Partial<WritePreview> = {},
): WritePreview => ({
  requestId: "job-1:mcp:1",
  workId: "work-1",
  spaceId: "space-a",
  operation: "ugoite.save",
  action: "create",
  form: "Note",
  summary: "Fields (new Entry values): title: text (17 chars).",
  ...overrides,
});

describe("KonasePanel Space authority", () => {
  beforeEach(() => {
    setLocale("en");
    getSpaceMock.mockReset();
    authorizeMock.mockReset();
    listFormsMock.mockReset();
    queryEntriesMock.mockReset();
    listFormsMock.mockResolvedValue([]);
    queryEntriesMock.mockResolvedValue({ rows: [], has_more: false });
    hostInstances.length = 0;
  });

  it("starts MCP authorization from the rendered Space and drops the host when the Space changes", async () => {
    const [spaceId, setSpaceId] = createSignal("space-a");
    getSpaceMock.mockResolvedValue({
      space_uid: "space-a-uid",
      name: "Space A",
      created_at: "",
    });
    authorizeMock.mockImplementation(
      async (options: BrowserMcpAuthorizationOptions) => {
        options.onApprovalRequired?.({
          verificationUriComplete: `${location.origin}/device?user_code=ABCD`,
          userCode: "ABCD",
        });
        return {
          accessToken: "space-a-token",
          endpoint: "/mcp",
          resource: `${location.origin}/mcp`,
          spaceUid: options.spaceUid,
        };
      },
    );

    render(() => (
      <>
        <KonasePanel spaceId={spaceId()} />
        <button type="button" onClick={() => setSpaceId("space-b")}>
          Switch Space
        </button>
      </>
    ));

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));

    await waitFor(() =>
      expect(authorizeMock).toHaveBeenCalledWith(
        expect.objectContaining({
          spaceUid: "space-a-uid",
          deviceName: "Ugoite Browser Konase (space-a)",
        }),
      )
    );
    expect(getSpaceMock).toHaveBeenCalledWith("space-a");
    expect(screen.getByPlaceholderText(/Ask Konase/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Switch Space" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Connect Ugoite MCP" }))
        .toBeInTheDocument()
    );
    expect(screen.queryByPlaceholderText(/Ask Konase/)).not.toBeInTheDocument();
  });

  it("shows connection progress while the current Space is being resolved", async () => {
    const spaceDeferred = createDeferred<{
      space_uid: string;
      name: string;
      created_at: string;
    }>();
    mockConnection();
    getSpaceMock.mockReturnValue(spaceDeferred.promise);
    render(() => <KonasePanel spaceId="space-a" />);

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));

    const connectButton = screen.getByRole("button", {
      name: "Connect Ugoite MCP",
    });
    expect(connectButton).toBeDisabled();
    expect(connectButton).toHaveAttribute("aria-busy", "true");

    spaceDeferred.resolve({
      space_uid: "space-a-uid",
      name: "Space A",
      created_at: "",
    });
    await waitFor(() => expect(hostInstances).toHaveLength(1));
  });

  it("announces Form candidate loading and the empty Forms state", async () => {
    const formsDeferred = createDeferred<unknown[]>();
    mockConnection();
    listFormsMock.mockReturnValue(formsDeferred.promise);
    render(() => <KonasePanel spaceId="space-a" />);

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));

    expect(await screen.findByText("Loading candidates…")).toHaveAttribute(
      "role",
      "status",
    );
    expect(
      screen.queryByText("No Forms are available in this Space."),
    ).not.toBeInTheDocument();
    formsDeferred.resolve([]);
    expect(
      await screen.findByText("No Forms are available in this Space."),
    ).toBeVisible();
    expect(screen.queryByText("Loading candidates…")).not.toBeInTheDocument();
  });

  it("announces Entry candidate loading while a search is pending", async () => {
    const entriesDeferred = createDeferred<{
      rows: Array<{
        id: string;
        form_id: string;
        revision_id: string;
        created_at_micros: number;
        updated_at_micros: number;
        preview: string;
      }>;
      has_more: boolean;
    }>();
    mockConnection();
    queryEntriesMock.mockReturnValue(entriesDeferred.promise);
    render(() => <KonasePanel spaceId="space-a" />);

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await screen.findByRole("button", { name: "Search Entries" });
    fireEvent.click(screen.getByRole("button", { name: "Search Entries" }));

    expect(await screen.findByText("Loading candidates…")).toHaveAttribute(
      "role",
      "status",
    );
    entriesDeferred.resolve({
      rows: [{
        id: "entry-a",
        form_id: "form-a",
        revision_id: "rev-a",
        created_at_micros: 1,
        updated_at_micros: 1,
        preview: "Quarterly plan",
      }],
      has_more: false,
    });

    expect(
      await screen.findByRole("checkbox", {
        name: "Quarterly plan",
        exact: true,
      }),
    ).toBeVisible();
    expect(screen.queryByText("Loading candidates…")).not.toBeInTheDocument();
  });

  it("shows an inline alert when Form candidates fail to load", async () => {
    mockConnection();
    listFormsMock.mockRejectedValue(new Error("Form service detail"));
    render(() => <KonasePanel spaceId="space-a" />);

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not load current Space candidates.");
    expect(
      screen.queryByText("No Forms are available in this Space."),
    ).not.toBeInTheDocument();
  });

  it("shows an inline alert when Entry candidates fail to load", async () => {
    mockConnection();
    queryEntriesMock.mockRejectedValue(
      new Error("Entry service detail"),
    );
    render(() => <KonasePanel spaceId="space-a" />);

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await screen.findByRole("button", { name: "Search Entries" });
    fireEvent.click(screen.getByRole("button", { name: "Search Entries" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not load current Space candidates.");
  });

  it("does not configure when the current Space has no server UID", async () => {
    getSpaceMock.mockResolvedValue({
      id: "space-a",
      name: "Space A",
      created_at: "",
    });
    render(() => <KonasePanel spaceId="space-a" />);

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));

    await waitFor(() => expect(authorizeMock).not.toHaveBeenCalled());
    // Typed INVALID_INPUT routes through the shared localizer with Space
    // identity detail, not a generic error.
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/request is invalid/i)
    );
    expect(screen.getByRole("alert")).toHaveTextContent(/space_identity/);
  });

  it("shows an unchanged Knowledge outcome when the model only answers", async () => {
    mockConnection();
    listFormsMock.mockResolvedValue([]);
    render(() => <KonasePanel spaceId="space-a" />);

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    const host = hostInstances[0];
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Save this" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(host.submitDeferreds).toHaveLength(1));
    host.submitDeferreds[0].resolve({
      ...fakeTurn("Model answered"),
      undoAvailable: false,
      knowledge: "unchanged",
    });

    await waitFor(() =>
      expect(screen.getByText("Knowledge: unchanged")).toBeInTheDocument()
    );
    expect(screen.queryByRole("button", { name: "Undo" })).not
      .toBeInTheDocument();
  });

  it("REQ-UX-LIST-001: keeps resource URIs internal and shows human-readable Context preview labels", async () => {
    mockConnection();
    listFormsMock.mockResolvedValue([
      { id: "form-a", name: "Note", version: 1, template: "", fields: {} },
      {
        id: "form-unnamed",
        name: "form-unnamed",
        version: 1,
        template: "",
        fields: {},
      },
    ]);
    queryEntriesMock.mockResolvedValueOnce({
      rows: [
        {
          id: "entry-a",
          form_id: "form-a",
          revision_id: "rev-a",
          created_at_micros: 1,
          updated_at_micros: 1,
          preview: "Selected entry",
        },
        {
          id: "entry-unselected",
          form_id: "form-a",
          revision_id: "rev-b",
          created_at_micros: 2,
          updated_at_micros: 2,
          preview: "Other entry",
        },
        {
          id: "entry-unnamed",
          form_id: "form-a",
          revision_id: "rev-empty",
          created_at_micros: 4,
          updated_at_micros: 4,
          preview: "entry-unnamed",
        },
      ],
      has_more: true,
      next: "page-2",
    }).mockResolvedValueOnce({
      rows: [
        {
          id: "entry-b",
          form_id: "form-a",
          revision_id: "rev-c",
          created_at_micros: 3,
          updated_at_micros: 3,
          preview: "Second page entry",
        },
      ],
      has_more: false,
    });
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    await waitFor(() =>
      expect(screen.getByRole("checkbox", { name: "Note", exact: true }))
        .toHaveAccessibleName("Note")
    );
    expect(screen.getByRole("checkbox", { name: "Form", exact: true }))
      .toBeInTheDocument();
    expect(screen.queryByText("form-unnamed")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("checkbox", { name: "Note", exact: true }),
    );
    const selectedResources = screen.getByRole("list", {
      name: "Selected resources",
    });
    expect(within(selectedResources).getByText("Note")).toBeInTheDocument();
    expect(within(selectedResources).queryByText(/form-a/)).not
      .toBeInTheDocument();
    expect(screen.queryByText("form-a")).not.toBeInTheDocument();
    expect(screen.queryByText("ugoite://form/form-a")).not.toBeInTheDocument();
    expect(
      within(selectedResources).getByRole("button", {
        name: "Remove Note from selected resources",
      }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Search Entries" }));
    await waitFor(() =>
      expect(screen.getByRole("checkbox", {
        name: "Selected entry",
        exact: true,
      })).toHaveAccessibleName("Selected entry")
    );
    expect(screen.getByRole("checkbox", { name: "Entry", exact: true }))
      .toBeInTheDocument();
    expect(screen.queryByText("entry-unnamed")).not.toBeInTheDocument();
    expect(queryEntriesMock).toHaveBeenCalledWith(
      "space-a",
      expect.objectContaining({
        projection: { kind: "preview" },
        limit: 20,
      }),
      expect.any(AbortSignal),
    );
    fireEvent.click(screen.getByRole("checkbox", {
      name: "Selected entry",
      exact: true,
    }));
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() =>
      expect(screen.getByRole("checkbox", {
        name: "Second page entry",
        exact: true,
      }))
        .toBeInTheDocument()
    );
    expect(queryEntriesMock).toHaveBeenLastCalledWith(
      "space-a",
      expect.objectContaining({ after: "page-2", limit: 20 }),
      expect.any(AbortSignal),
    );
    fireEvent.click(screen.getByRole("checkbox", {
      name: "Second page entry",
      exact: true,
    }));
    expect(within(selectedResources).getByText("Selected entry"))
      .toBeInTheDocument();
    expect(within(selectedResources).getByText("Second page entry"))
      .toBeInTheDocument();
    expect(within(selectedResources).queryByText(/entry-[ab]/)).not
      .toBeInTheDocument();
    expect(screen.queryByText("entry-a")).not.toBeInTheDocument();
    expect(screen.queryByText("entry-b")).not.toBeInTheDocument();
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Explain these resources" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Preview selected Context" }),
    );

    const host = hostInstances[0];
    await waitFor(() => expect(host.previewDeferreds).toHaveLength(1));
    expect(host.selectedUriCalls).toEqual([[
      "ugoite://form/form-a",
      "ugoite://entry/entry-a",
      "ugoite://entry/entry-b",
    ]]);
    expect(host.submitDeferreds).toHaveLength(0);
    host.previewDeferreds[0].resolve({
      id: "preview-a",
      spaceId: "space-a",
      selectedUris: [
        "ugoite://form/form-a",
        "ugoite://entry/entry-a",
        "ugoite://entry/entry-b",
      ],
      admission: [
        { uri: "ugoite://form/form-a", status: "included" },
        {
          uri: "ugoite://entry/entry-a",
          status: "truncated",
          reason: "projection_compacted",
        },
        { uri: "ugoite://entry/entry-b", status: "included" },
      ],
      resources: [
        { uri: "ugoite://form/form-a", content: "normalized Form projection" },
        {
          uri: "ugoite://entry/entry-a",
          content: "normalized selected Entry projection",
        },
        {
          uri: "ugoite://entry/entry-b",
          content: "normalized second page Entry projection",
        },
      ],
    });
    await waitFor(() =>
      expect(screen.getByText("normalized Form projection")).toBeInTheDocument()
    );
    expect(screen.getByText("Note", { selector: "strong" }))
      .toBeInTheDocument();
    expect(screen.getByText("Selected entry", { selector: "strong" }))
      .toBeInTheDocument();
    expect(screen.getByText("Second page entry", { selector: "strong" }))
      .toBeInTheDocument();
    expect(screen.queryByText("ugoite://form/form-a")).not.toBeInTheDocument();
    expect(screen.queryByText("ugoite://entry/entry-a")).not
      .toBeInTheDocument();
    expect(screen.queryByText("ugoite://entry/entry-b")).not
      .toBeInTheDocument();
    const previewHeading = screen.getByRole("heading", {
      name: "Review Context before sending",
    });
    await waitFor(() => expect(previewHeading).toHaveFocus());
    expect(screen.getByText(/Selected Knowledge is untrusted data/))
      .toHaveAttribute("role", "status");
    expect(screen.getByRole("button", { name: "Send this Context" }))
      .toBeInTheDocument();
    expect(screen.queryByText("entry-unselected")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Send this Context" }));
    await waitFor(() => expect(host.sendDeferreds).toHaveLength(1));
    expect(host.submitDeferreds).toHaveLength(0);
    host.sendDeferreds[0].resolve(fakeTurn("Context sent"));
    await waitFor(() =>
      expect(screen.getByText("Context sent")).toBeInTheDocument()
    );
  });

  it("REQ-UX-LIST-003: disambiguates repeated Entry previews across pages", async () => {
    mockConnection();
    const candidate = (id: string, preview = "Shared preview") => ({
      id,
      form_id: "form-a",
      revision_id: `revision-${id}`,
      created_at_micros: 1,
      updated_at_micros: 1,
      preview,
    });
    queryEntriesMock.mockResolvedValueOnce({
      rows: [candidate("entry-a"), candidate("entry-b")],
      has_more: true,
      next: "page-2",
    }).mockResolvedValueOnce({
      rows: [
        candidate("entry-c"),
        candidate("entry-d", "Shared preview · Entry 1"),
      ],
      has_more: false,
    });

    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Search Entries" }));

    const firstCandidate = await screen.findByRole("checkbox", {
      name: "Shared preview · Entry 1",
      exact: true,
    });
    const secondCandidate = screen.getByRole("checkbox", {
      name: "Shared preview · Entry 2",
      exact: true,
    });
    expect(document.body.textContent).not.toMatch(/entry-[abc]/);
    fireEvent.click(firstCandidate);
    fireEvent.click(secondCandidate);

    const selectedResources = screen.getByRole("list", {
      name: "Selected resources",
    });
    expect(within(selectedResources).getByText("Shared preview · Entry 1"))
      .toBeInTheDocument();
    expect(
      within(selectedResources).getByRole("button", {
        name: "Remove Shared preview · Entry 1 from selected resources",
      }),
    ).toBeInTheDocument();
    expect(within(selectedResources).getByText("Shared preview · Entry 2"))
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    const thirdCandidate = await screen.findByRole("checkbox", {
      name: "Entry 3 · Shared preview",
      exact: true,
    });
    const fourthCandidate = screen.getByRole("checkbox", {
      name: "Entry 4 · Shared preview · Entry 1",
      exact: true,
    });
    expect(within(selectedResources).getByText("Entry 1 · Shared preview"))
      .toBeInTheDocument();
    expect(within(selectedResources).getByText("Entry 2 · Shared preview"))
      .toBeInTheDocument();
    expect(
      within(selectedResources).getByRole("button", {
        name: "Remove Entry 1 · Shared preview from selected resources",
      }),
    ).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/entry-[abcd]/);
    fireEvent.click(thirdCandidate);
    expect(within(selectedResources).getByText("Entry 3 · Shared preview"))
      .toBeInTheDocument();
    fireEvent.click(
      within(selectedResources).getByRole("button", {
        name: "Remove Entry 3 · Shared preview from selected resources",
      }),
    );
    expect(within(selectedResources).queryByText("Entry 3 · Shared preview"))
      .not.toBeInTheDocument();
    expect(thirdCandidate).not.toBeChecked();
    expect(within(selectedResources).getByText("Entry 1 · Shared preview"))
      .toBeInTheDocument();
    expect(within(selectedResources).getByText("Entry 2 · Shared preview"))
      .toBeInTheDocument();
    fireEvent.click(fourthCandidate);
    expect(
      within(selectedResources).getByText(
        "Entry 4 · Shared preview · Entry 1",
      ),
    ).toBeInTheDocument();

    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Explain these resources" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Preview selected Context" }),
    );

    const host = hostInstances[0];
    await waitFor(() => expect(host.previewDeferreds).toHaveLength(1));
    const firstUri = "ugoite://entry/entry-a";
    const secondUri = "ugoite://entry/entry-b";
    const fourthUri = "ugoite://entry/entry-d";
    expect(host.selectedUriCalls).toEqual([[firstUri, secondUri, fourthUri]]);
    host.previewDeferreds[0].resolve({
      id: "preview-duplicates",
      spaceId: "space-a",
      selectedUris: [firstUri, secondUri, fourthUri],
      admission: [
        { uri: firstUri, status: "included" },
        { uri: secondUri, status: "included" },
        { uri: fourthUri, status: "included" },
      ],
      resources: [
        { uri: firstUri, content: "First Entry projection" },
        { uri: secondUri, content: "Second Entry projection" },
        { uri: fourthUri, content: "Fourth Entry projection" },
      ],
    });
    await waitFor(() =>
      expect(screen.getByText("Fourth Entry projection")).toBeInTheDocument()
    );
  });

  it("restores keyboard focus to the preview trigger when the preview is cancelled", async () => {
    mockConnection();
    listFormsMock.mockResolvedValue([
      { id: "form-a", name: "Note", version: 1, template: "", fields: {} },
    ]);
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    await waitFor(() =>
      expect(screen.getByLabelText("Note")).toBeInTheDocument()
    );
    fireEvent.click(screen.getByLabelText("Note"));
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Explain this Form" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Preview selected Context" }),
    );
    const host = hostInstances[0];
    await waitFor(() => expect(host.previewDeferreds).toHaveLength(1));
    host.previewDeferreds[0].resolve({
      id: "preview-cancel",
      spaceId: "space-a",
      selectedUris: ["ugoite://form/form-a"],
      admission: [{ uri: "ugoite://form/form-a", status: "included" }],
      resources: [{ uri: "ugoite://form/form-a", content: "Form projection" }],
    });

    const cancelButton = await screen.findByRole("button", {
      name: "Cancel preview",
    });
    const previewTrigger = screen.getByRole("button", {
      name: "Preview selected Context",
    });
    await waitFor(() =>
      expect(screen.getByRole("heading", {
        name: "Review Context before sending",
      })).toHaveFocus()
    );
    cancelButton.focus();
    fireEvent.click(cancelButton);

    await waitFor(() => expect(previewTrigger).toHaveFocus());
    expect(screen.queryByRole("button", { name: "Send this Context" }))
      .not.toBeInTheDocument();
  });

  it("moves focus to the new Space connection action when an open preview is invalidated", async () => {
    mockConnection();
    listFormsMock.mockResolvedValue([
      { id: "form-a", name: "Note", version: 1, template: "", fields: {} },
    ]);
    const [spaceId, setSpaceId] = createSignal("space-a");
    render(() => <KonasePanel spaceId={spaceId()} />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    await waitFor(() =>
      expect(screen.getByLabelText("Note")).toBeInTheDocument()
    );
    fireEvent.click(screen.getByLabelText("Note"));
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Explain this Form" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Preview selected Context" }),
    );
    const oldHost = hostInstances[0];
    await waitFor(() => expect(oldHost.previewDeferreds).toHaveLength(1));
    oldHost.previewDeferreds[0].resolve({
      id: "preview-space-change",
      spaceId: "space-a",
      selectedUris: ["ugoite://form/form-a"],
      admission: [{ uri: "ugoite://form/form-a", status: "included" }],
      resources: [{ uri: "ugoite://form/form-a", content: "Form projection" }],
    });
    await waitFor(() =>
      expect(screen.getByRole("heading", {
        name: "Review Context before sending",
      })).toHaveFocus()
    );

    setSpaceId("space-b");

    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Konase" }))
        .toHaveFocus()
    );
    expect(screen.queryByText("Form projection")).not.toBeInTheDocument();
    expect(oldHost.disposed).toBe(true);
  });

  it("drops a late Context preview when the Panel moves to another Space", async () => {
    mockConnection();
    listFormsMock.mockResolvedValue([
      { id: "form-a", name: "Note", version: 1, template: "", fields: {} },
    ]);
    const [spaceId, setSpaceId] = createSignal("space-a");
    render(() => (
      <>
        <KonasePanel spaceId={spaceId()} />
        <button type="button" onClick={() => setSpaceId("space-b")}>
          Switch Space
        </button>
      </>
    ));
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    await waitFor(() =>
      expect(screen.getByLabelText("Note")).toBeInTheDocument()
    );
    fireEvent.click(screen.getByLabelText("Note"));
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Explain this Form" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Preview selected Context" }),
    );
    const oldHost = hostInstances[0];
    await waitFor(() => expect(oldHost.previewDeferreds).toHaveLength(1));

    fireEvent.click(screen.getByRole("button", { name: "Switch Space" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Connect Ugoite MCP" }))
        .toBeInTheDocument()
    );
    oldHost.previewDeferreds[0].resolve({
      id: "stale-preview",
      spaceId: "space-a",
      selectedUris: ["ugoite://form/form-a"],
      admission: [{ uri: "ugoite://form/form-a", status: "included" }],
      resources: [{ uri: "ugoite://form/form-a", content: "stale Space data" }],
    });

    await waitFor(() =>
      expect(screen.queryByText("stale Space data")).not.toBeInTheDocument()
    );
    expect(screen.queryByRole("button", { name: "Send this Context" })).not
      .toBeInTheDocument();
    expect(oldHost.sendDeferreds).toHaveLength(0);
  });

  it("does not bind a credential if the Space changes during approval", async () => {
    const [spaceId, setSpaceId] = createSignal("space-a");
    let resolveAuthorization!: (credential: {
      accessToken: string;
      endpoint: string;
      resource: string;
      spaceUid: string;
    }) => void;
    getSpaceMock.mockResolvedValue({
      space_uid: "space-a-uid",
      name: "Space A",
      created_at: "",
    });
    authorizeMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveAuthorization = resolve;
        }),
    );

    render(() => (
      <>
        <KonasePanel spaceId={spaceId()} />
        <button type="button" onClick={() => setSpaceId("space-b")}>
          Switch Space
        </button>
      </>
    ));

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(authorizeMock).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Switch Space" }));
    resolveAuthorization({
      accessToken: "space-a-token",
      endpoint: "/mcp",
      resource: `${location.origin}/mcp`,
      spaceUid: "space-a-uid",
    });

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Connect Ugoite MCP" }))
        .toBeInTheDocument()
    );
    expect(screen.queryByPlaceholderText(/Ask Konase/)).not.toBeInTheDocument();
  });

  it("drops late completion and progress without touching the new Space state", async () => {
    mockConnection();
    const [spaceId, setSpaceId] = createSignal("space-a");

    render(() => (
      <>
        <KonasePanel spaceId={spaceId()} />
        <button type="button" onClick={() => setSpaceId("space-b")}>
          Switch Space
        </button>
      </>
    ));

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));

    const spaceAHost = hostInstances[0];
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Ask Space A" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(spaceAHost.submitDeferreds).toHaveLength(1));

    fireEvent.click(screen.getByRole("button", { name: "Switch Space" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Connect Ugoite MCP" }))
        .toBeInTheDocument()
    );

    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(2));
    const spaceBHost = hostInstances[1];
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Ask Space B" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(spaceBHost.submitDeferreds).toHaveLength(1));

    spaceAHost.emitProgress({ kind: "complete", summary: "old completion" });
    spaceAHost.submitDeferreds[0].resolve(fakeTurn("old completion"));

    await waitFor(() => {
      const run = screen.getByRole("button", { name: "Run" });
      expect(run).toBeInTheDocument();
      expect(run).toHaveAttribute("aria-busy", "true");
    });
    expect(screen.getByPlaceholderText(/Ask Konase/)).toHaveValue(
      "Ask Space B",
    );
    expect(screen.queryByText("old completion")).not.toBeInTheDocument();
    expect(screen.queryByText("Completed")).not.toBeInTheDocument();

    spaceBHost.submitDeferreds[0].resolve(fakeTurn("Space B completion"));
    await waitFor(() =>
      expect(screen.getByText("Space B completion")).toBeInTheDocument()
    );
  });

  it("drops late errors from an obsolete Work", async () => {
    mockConnection();
    const [spaceId, setSpaceId] = createSignal("space-a");

    render(() => (
      <>
        <KonasePanel spaceId={spaceId()} />
        <button type="button" onClick={() => setSpaceId("space-b")}>
          Switch Space
        </button>
      </>
    ));

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    const spaceAHost = hostInstances[0];
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Ask Space A" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(spaceAHost.submitDeferreds).toHaveLength(1));

    fireEvent.click(screen.getByRole("button", { name: "Switch Space" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Connect Ugoite MCP" }))
        .toBeInTheDocument()
    );
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key-b" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(2));
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Ask Space B" },
    });

    spaceAHost.submitDeferreds[0].reject(new Error("old Work failed"));

    await waitFor(() =>
      expect(screen.getByPlaceholderText(/Ask Konase/)).toBeInTheDocument()
    );
    expect(screen.getByPlaceholderText(/Ask Konase/)).toHaveValue(
      "Ask Space B",
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("drops late undo completion without changing the new Work controls", async () => {
    mockConnection();
    const [spaceId, setSpaceId] = createSignal("space-a");

    render(() => (
      <>
        <KonasePanel spaceId={spaceId()} />
        <button type="button" onClick={() => setSpaceId("space-b")}>
          Switch Space
        </button>
      </>
    ));

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    const spaceAHost = hostInstances[0];
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Save in Space A" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(spaceAHost.submitDeferreds).toHaveLength(1));
    spaceAHost.submitDeferreds[0].resolve(fakeTurn("Space A saved"));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument()
    );

    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(spaceAHost.undoDeferreds).toHaveLength(1));

    fireEvent.click(screen.getByRole("button", { name: "Switch Space" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Connect Ugoite MCP" }))
        .toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(2));
    const spaceBHost = hostInstances[1];
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Save in Space B" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(spaceBHost.submitDeferreds).toHaveLength(1));
    spaceBHost.submitDeferreds[0].resolve(fakeTurn("Space B saved"));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument()
    );

    spaceAHost.undoDeferreds[0].resolve({ success: true });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument()
    );
    expect(screen.queryByText("Undone")).not.toBeInTheDocument();
  });

  it("shows an accessible write approval preview and does not mark MCP start as success", async () => {
    mockConnection();
    getSpaceMock.mockResolvedValue({
      space_uid: "space-a-uid",
      name: "Space A",
      created_at: "",
    });
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    const host = hostInstances[0];

    host.requestConfirmation(fakeWritePreview());
    const dialog = screen.getByRole("alertdialog");
    expect(dialog).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Approve this write?" }))
      .toBeInTheDocument();
    expect(within(dialog).getByText("Space A / Note")).toBeVisible();
    expect(within(dialog).getByText("space-a")).not.toBeVisible();
    expect(screen.getByText(/Fields \(new Entry values\)/))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Approve write" }));
    expect(host.resolvedConfirmations).toEqual([{
      requestId: "job-1:mcp:1",
      approved: true,
    }]);

    host.emitProgress({ kind: "mcp", operation: "ugoite.save" });
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(screen.getByText("MCP request started: ugoite.save"))
      .toBeInTheDocument();
    expect(screen.getByText("MCP request started: ugoite.save").textContent)
      .not.toContain("✓");
  });

  it("keeps update targets readable and reveals opaque IDs only on demand", async () => {
    mockConnection();
    getSpaceMock.mockResolvedValue({
      space_uid: "space-a-uid",
      name: "space-a",
      slug: "Project Notes",
      created_at: "",
    });
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));

    const host = hostInstances[0];
    host.requestConfirmation(fakeWritePreview({
      requestId: "job-update:mcp:1",
      spaceId: "space-private-opaque-id",
      action: "update",
      entryId: "entry-private-opaque-id",
      entryIdLabel: "entry-private-opaque-id",
    }));

    const dialog = screen.getByRole("alertdialog");
    expect(within(dialog).getByText("Project Notes / Note / Existing Entry"))
      .toBeVisible();
    expect(within(dialog).getByText("space-private-opaque-id"))
      .not.toBeVisible();
    expect(within(dialog).getByText("entry-private-opaque-id"))
      .not.toBeVisible();

    fireEvent.click(within(dialog).getByText("Technical details"));
    expect(within(dialog).getByText("space-private-opaque-id")).toBeVisible();
    expect(within(dialog).getByText("entry-private-opaque-id")).toBeVisible();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Approve write" }),
    );
    expect(host.resolvedConfirmations).toEqual([{
      requestId: "job-update:mcp:1",
      approved: true,
    }]);
  });

  it("uses the current candidate label for the exact updated Entry", async () => {
    mockConnection();
    getSpaceMock.mockResolvedValue({
      space_uid: "space-a-uid",
      name: "Project Notes",
      created_at: "",
    });
    queryEntriesMock.mockResolvedValueOnce({
      rows: [
        {
          id: "entry-other",
          form_id: "form-a",
          revision_id: "rev-other",
          created_at_micros: 1,
          updated_at_micros: 1,
          preview: "Onboarding notes",
        },
        {
          id: "entry-target",
          form_id: "form-a",
          revision_id: "rev-target",
          created_at_micros: 2,
          updated_at_micros: 2,
          preview: "Quarterly plan",
        },
      ],
      has_more: false,
    });
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Search Entries" }));
    await screen.findByRole("checkbox", {
      name: "Quarterly plan",
      exact: true,
    });

    const host = hostInstances[0];
    host.requestConfirmation(fakeWritePreview({
      action: "update",
      entryId: "entry-target",
      entryIdLabel: "entry-target",
    }));

    const dialog = screen.getByRole("alertdialog");
    expect(
      within(dialog).getByText(
        "Project Notes / Note / Quarterly plan",
      ),
    ).toBeVisible();
    expect(within(dialog).getByText("entry-target")).not.toBeVisible();

    fireEvent.click(within(dialog).getByText("Technical details"));
    expect(within(dialog).getByText("entry-target")).toBeVisible();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Approve write" }),
    );
    expect(host.resolvedConfirmations).toEqual([{
      requestId: "job-1:mcp:1",
      approved: true,
    }]);
  });

  it("uses a selected Entry label after paging away from its candidate row", async () => {
    mockConnection();
    getSpaceMock.mockResolvedValue({
      space_uid: "space-a-uid",
      name: "Project Notes",
      created_at: "",
    });
    queryEntriesMock.mockResolvedValueOnce({
      rows: [
        {
          id: "entry-selected",
          form_id: "form-a",
          revision_id: "rev-selected",
          created_at_micros: 1,
          updated_at_micros: 1,
          preview: "Quarterly plan",
        },
      ],
      has_more: true,
      next: "page-2",
    }).mockResolvedValueOnce({
      rows: [
        {
          id: "entry-current-page",
          form_id: "form-a",
          revision_id: "rev-current-page",
          created_at_micros: 2,
          updated_at_micros: 2,
          preview: "Onboarding notes",
        },
      ],
      has_more: false,
    });
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Search Entries" }));
    const selectedEntry = await screen.findByRole("checkbox", {
      name: "Quarterly plan",
      exact: true,
    });
    fireEvent.click(selectedEntry);
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await screen.findByRole("checkbox", {
      name: "Onboarding notes",
      exact: true,
    });

    const host = hostInstances[0];
    host.requestConfirmation(fakeWritePreview({
      action: "update",
      entryId: "entry-selected",
      entryIdLabel: "entry-selected",
    }));

    const dialog = screen.getByRole("alertdialog");
    expect(
      within(dialog).getByText(
        "Project Notes / Note / Quarterly plan",
      ),
    ).toBeVisible();
    expect(within(dialog).getByText("entry-selected")).not.toBeVisible();
    expect(within(dialog).queryByText(/Onboarding notes/)).toBeNull();

    fireEvent.click(within(dialog).getByText("Technical details"));
    expect(within(dialog).getByText("entry-selected")).toBeVisible();
    fireEvent.click(within(dialog).getByText("Technical details"));
    expect(within(dialog).getByText("entry-selected")).not.toBeVisible();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Approve write" }),
    );
    expect(host.resolvedConfirmations).toEqual([{
      requestId: "job-1:mcp:1",
      approved: true,
    }]);

    host.requestConfirmation(fakeWritePreview({
      action: "update",
      entryId: "entry-unmatched",
      entryIdLabel: "entry-unmatched",
    }));
    const unmatchedDialog = screen.getByRole("alertdialog");
    expect(
      within(unmatchedDialog).getByText(
        "Project Notes / Note / Existing Entry",
      ),
    ).toBeVisible();
    expect(within(unmatchedDialog).queryByText(/Quarterly plan/)).toBeNull();
    expect(within(unmatchedDialog).queryByText(/Onboarding notes/)).toBeNull();
    expect(within(unmatchedDialog).getByText("entry-unmatched"))
      .not.toBeVisible();
    fireEvent.click(within(unmatchedDialog).getByText("Technical details"));
    expect(within(unmatchedDialog).getByText("entry-unmatched")).toBeVisible();
    fireEvent.click(
      within(unmatchedDialog).getByRole("button", { name: "Deny write" }),
    );
  });

  it("denies a pending write explicitly", async () => {
    mockConnection();
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    const host = hostInstances[0];
    host.requestConfirmation(fakeWritePreview());

    fireEvent.click(screen.getByRole("button", { name: "Deny write" }));

    expect(host.resolvedConfirmations).toEqual([{
      requestId: "job-1:mcp:1",
      approved: false,
    }]);
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("keeps Undo available after a confirmed save followed by a denied write", async () => {
    mockConnection();
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    const host = hostInstances[0];
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Save two notes" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(host.submitDeferreds).toHaveLength(1));
    host.submitDeferreds[0].reject(
      new KonaseWorkFailure(new Error("Konase write was not approved"), {
        workId: "work-a",
        jobId: "job-a",
        knowledge: "saved",
        undoAvailable: true,
      }),
    );

    expect(await screen.findByText("Work stopped after a confirmed save."))
      .toBeInTheDocument();
    expect(screen.getByText("Knowledge: saved")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });

  it("cancels and disposes the old Host before switching Spaces with approval open", async () => {
    mockConnection();
    const [spaceId, setSpaceId] = createSignal("space-a");
    render(() => (
      <>
        <KonasePanel spaceId={spaceId()} />
        <button type="button" onClick={() => setSpaceId("space-b")}>
          Switch Space
        </button>
      </>
    ));
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    const oldHost = hostInstances[0];
    oldHost.requestConfirmation(fakeWritePreview());

    fireEvent.click(screen.getByRole("button", { name: "Switch Space" }));

    await waitFor(() => expect(oldHost.disposed).toBe(true));
    expect(oldHost.cancelledConfirmations).toEqual(["job-1:mcp:1"]);
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("cancels and disposes a pending approval on unmount", async () => {
    mockConnection();
    const view = render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    const host = hostInstances[0];
    host.requestConfirmation(fakeWritePreview());

    view.unmount();

    expect(host.cancelledConfirmations).toEqual(["job-1:mcp:1"]);
    expect(host.disposed).toBe(true);
  });
});
