import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
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
  previewSelectedContext(prompt: string, uris: string[]): Promise<SelectedContextPreview>;
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

const { getSpaceMock, authorizeMock, listFormsMock, queryEntriesMock, hostInstances, createDeferred } = vi
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
  KonaseWriteDeniedError: class extends Error {},
  KonaseMutationUnconfirmedError: class extends Error {},
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

    previewSelectedContext(_prompt: string, uris: string[]): Promise<SelectedContextPreview> {
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

const fakeWritePreview = (): WritePreview => ({
  requestId: "job-1:mcp:1",
  workId: "work-1",
  spaceId: "space-a",
  operation: "ugoite.save",
  action: "create",
  form: "Note",
  summary:
    "Create in Space space-a; Form Note. Fields (new Entry values): title: text (17 chars).",
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

  it("reads only selected Form and Entry candidates, previews normalized Context, then waits for send", async () => {
    mockConnection();
    listFormsMock.mockResolvedValue([
      { id: "form-a", name: "Note", version: 1, template: "", fields: {} },
    ]);
    queryEntriesMock.mockResolvedValueOnce({
      rows: [
        { id: "entry-a", form_id: "form-a", revision_id: "rev-a", created_at_micros: 1, updated_at_micros: 1, preview: "Selected entry" },
        { id: "entry-unselected", form_id: "form-a", revision_id: "rev-b", created_at_micros: 2, updated_at_micros: 2, preview: "Other entry" },
      ],
      has_more: true,
      next: "page-2",
    }).mockResolvedValueOnce({
      rows: [
        { id: "entry-b", form_id: "form-a", revision_id: "rev-c", created_at_micros: 3, updated_at_micros: 3, preview: "Second page entry" },
      ],
      has_more: false,
    });
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    await waitFor(() => expect(screen.getByLabelText("Note (form-a)")).toBeInTheDocument());
    fireEvent.click(screen.getByLabelText("Note (form-a)"));
    fireEvent.click(screen.getByRole("button", { name: "Search Entries" }));
    await waitFor(() => expect(screen.getByLabelText("Selected entry (entry-a)")).toBeInTheDocument());
    expect(queryEntriesMock).toHaveBeenCalledWith(
      "space-a",
      expect.objectContaining({
        projection: { kind: "preview" },
        limit: 20,
      }),
      expect.any(AbortSignal),
    );
    fireEvent.click(screen.getByLabelText("Selected entry (entry-a)"));
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() => expect(screen.getByLabelText("Second page entry (entry-b)")).toBeInTheDocument());
    expect(queryEntriesMock).toHaveBeenLastCalledWith(
      "space-a",
      expect.objectContaining({ after: "page-2", limit: 20 }),
      expect.any(AbortSignal),
    );
    fireEvent.click(screen.getByLabelText("Second page entry (entry-b)"));
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Explain these resources" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Preview selected Context" }));

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
      selectedUris: ["ugoite://form/form-a", "ugoite://entry/entry-a", "ugoite://entry/entry-b"],
      admission: [
        { uri: "ugoite://form/form-a", status: "included" },
        { uri: "ugoite://entry/entry-a", status: "truncated", reason: "projection_compacted" },
        { uri: "ugoite://entry/entry-b", status: "included" },
      ],
      resources: [
        { uri: "ugoite://form/form-a", content: "normalized Form projection" },
        { uri: "ugoite://entry/entry-a", content: "normalized selected Entry projection" },
        { uri: "ugoite://entry/entry-b", content: "normalized second page Entry projection" },
      ],
    });
    await waitFor(() => expect(screen.getByText("normalized Form projection")).toBeInTheDocument());
    expect(screen.queryByText("entry-unselected")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Send this Context" }));
    await waitFor(() => expect(host.sendDeferreds).toHaveLength(1));
    expect(host.submitDeferreds).toHaveLength(0);
    host.sendDeferreds[0].resolve(fakeTurn("Context sent"));
    await waitFor(() => expect(screen.getByText("Context sent")).toBeInTheDocument());
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
    await waitFor(() => expect(screen.getByLabelText("Note (form-a)")).toBeInTheDocument());
    fireEvent.click(screen.getByLabelText("Note (form-a)"));
    fireEvent.input(screen.getByPlaceholderText(/Ask Konase/), {
      target: { value: "Explain this Form" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Preview selected Context" }));
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

    await waitFor(() => expect(screen.queryByText("stale Space data")).not.toBeInTheDocument());
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
    render(() => <KonasePanel spaceId="space-a" />);
    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(hostInstances).toHaveLength(1));
    const host = hostInstances[0];

    host.requestConfirmation(fakeWritePreview());
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Approve this write?" }))
      .toBeInTheDocument();
    expect(screen.getByText("space-a / Note")).toBeInTheDocument();
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
