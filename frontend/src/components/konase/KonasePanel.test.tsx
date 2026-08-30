import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { KonasePanel } from "./KonasePanel";
import type { BrowserMcpAuthorizationOptions } from "~/lib/konase/browser-mcp-auth";

type FakeTurn = {
  outcome: { job_id: string; summary: string; meaningful: boolean };
  workId: string;
  undoAvailable: boolean;
  knowledge: "unchanged" | "saved" | "write_failed";
};

type FakeProgress =
  | { kind: "model" }
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
  undoDeferreds: Array<Deferred<{ success: boolean }>>;
  listeners: Array<(progress: FakeProgress) => void>;
  submit(prompt: string): Promise<FakeTurn>;
  undo(workId: string): Promise<{ success: boolean }>;
  subscribeProgress(listener: (progress: FakeProgress) => void): () => void;
  emitProgress(progress: FakeProgress): void;
};

const { getSpaceMock, authorizeMock, hostInstances, createDeferred } = vi
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
      hostInstances: [] as FakeKonaseHost[],
      createDeferred,
    };
  });

vi.mock("~/lib/konase/host", () => ({
  KonaseHost: class {
    readonly submitDeferreds: Array<Deferred<FakeTurn>> = [];
    readonly undoDeferreds: Array<Deferred<{ success: boolean }>> = [];
    readonly listeners: Array<(progress: FakeProgress) => void> = [];

    constructor(_options: unknown) {
      hostInstances.push(this);
    }

    submit(_prompt: string): Promise<FakeTurn> {
      const deferred = createDeferred<FakeTurn>();
      this.submitDeferreds.push(deferred);
      return deferred.promise;
    }

    undo(_workId: string): Promise<{ success: boolean }> {
      const deferred = createDeferred<{ success: boolean }>();
      this.undoDeferreds.push(deferred);
      return deferred.promise;
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
}));
vi.mock("~/lib/konase/browser-mcp-auth", () => ({
  authorizeBrowserMcp: authorizeMock,
}));

const mockConnection = () => {
  getSpaceMock.mockImplementation(async (spaceId: string) => ({
    id: spaceId,
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

describe("KonasePanel Space authority", () => {
  beforeEach(() => {
    setLocale("en");
    getSpaceMock.mockReset();
    authorizeMock.mockReset();
    hostInstances.length = 0;
  });

  it("starts MCP authorization from the rendered Space and drops the host when the Space changes", async () => {
    const [spaceId, setSpaceId] = createSignal("space-a");
    getSpaceMock.mockResolvedValue({
      id: "space-a",
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
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Konase could not complete the Work.",
    );
  });

  it("shows an unchanged Knowledge outcome when the model only answers", async () => {
    mockConnection();
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
    expect(screen.queryByRole("button", { name: "Undo" })).not.toBeInTheDocument();
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
      id: "space-a",
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

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Working..." }))
        .toBeInTheDocument()
    );
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
});
