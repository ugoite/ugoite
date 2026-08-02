import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal, Show } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AssetField } from "./AssetField";
import { setLocale } from "~/lib/i18n";
import { assetApi } from "~/lib/ugoite-client";
import {
  serializeAssetReference,
  serializeAssetReferenceList,
} from "~/lib/asset-reference";
import { createAssetFieldState } from "~/lib/asset-field-state";
import type { AssetReference } from "~/lib/types";

vi.mock("~/lib/ugoite-client", () => ({
  assetApi: {
    upload: vi.fn(),
    read: vi.fn(),
  },
}));

const first: AssetReference = {
  asset_id: "01900000-0000-7000-8000-000000000001",
  name: "first.txt",
  media_type: "text/plain",
  size_bytes: 10,
  sha256: "a".repeat(64),
};

const second: AssetReference = {
  asset_id: "01900000-0000-7000-8000-000000000002",
  name: "second.txt",
  media_type: "text/plain",
  size_bytes: 20,
  sha256: "b".repeat(64),
};

const queuedFirst: AssetReference = {
  ...first,
  asset_id: "01900000-0000-7000-8000-000000000011",
  name: "queued-first.txt",
};

const queuedSecond: AssetReference = {
  ...second,
  asset_id: "01900000-0000-7000-8000-000000000012",
  name: "queued-second.txt",
};

beforeEach(() => {
  vi.resetAllMocks();
  setLocale("en");
});

describe("AssetField", () => {
  it("uploads a scalar reference without claiming Entry persistence", async () => {
    (assetApi.upload as ReturnType<typeof vi.fn>).mockResolvedValue(first);
    const [value, setValue] = createSignal("");

    render(() => (
      <AssetField
        fieldId="thumbnail"
        fieldName="thumbnail"
        value={value()}
        persistedValue=""
        multiple={false}
        spaceId="default"
        onChange={setValue}
      />
    ));

    fireEvent.change(screen.getByLabelText("Choose file"), {
      target: {
        files: [new File(["data"], "first.txt", { type: "text/plain" })],
      },
    });

    await waitFor(() => expect(value()).toBe(serializeAssetReference(first)));
    expect(screen.getByText("Uploaded; entry not saved yet"))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
  });

  it("keeps typed-list order and removes only the selected reference", () => {
    const onChange = vi.fn();
    const [value, setValue] = createSignal(
      serializeAssetReferenceList([first, second]),
    );
    const change = (next: string) => {
      setValue(next);
      onChange(next);
    };

    render(() => (
      <AssetField
        fieldId="documents"
        fieldName="documents"
        value={value()}
        persistedValue={serializeAssetReferenceList([first, second])}
        multiple={true}
        spaceId="default"
        entryId="entry-1"
        formName="Contracts"
        onChange={change}
      />
    ));

    fireEvent.click(screen.getAllByRole("button", { name: / up$/ })[1]);
    expect(onChange).toHaveBeenLastCalledWith(
      serializeAssetReferenceList([second, first]),
    );
    fireEvent.click(screen.getAllByRole("button", { name: /^Remove/ })[0]);
    expect(onChange).toHaveBeenLastCalledWith(
      serializeAssetReferenceList([first]),
    );
  });

  it("replaces an individual typed-list item by Asset ID", async () => {
    const replacement = { ...second, name: "replacement.txt", size_bytes: 30 };
    (assetApi.upload as ReturnType<typeof vi.fn>).mockResolvedValue(
      replacement,
    );
    const [value, setValue] = createSignal(
      serializeAssetReferenceList([first, second]),
    );

    render(() => (
      <AssetField
        fieldId="documents"
        fieldName="documents"
        value={value()}
        persistedValue={serializeAssetReferenceList([first, second])}
        multiple={true}
        spaceId="default"
        entryId="entry-1"
        formName="Contracts"
        onChange={setValue}
      />
    ));

    fireEvent.change(screen.getAllByLabelText("Replace")[1], {
      target: {
        files: [
          new File(["replacement"], "replacement.txt", {
            type: "text/plain",
          }),
        ],
      },
    });

    await waitFor(() => {
      expect(value()).toBe(serializeAssetReferenceList([first, replacement]));
    });
  });

  it("cancels stale uploads when the editor generation changes", async () => {
    let resolveUpload: ((reference: AssetReference) => void) | undefined;
    (assetApi.upload as ReturnType<typeof vi.fn>).mockImplementation(
      () =>
        new Promise<AssetReference>((resolve) => {
          resolveUpload = resolve;
        }),
    );
    const [value, setValue] = createSignal("");
    const [generation, setGeneration] = createSignal(1);
    const state = createAssetFieldState();

    render(() => (
      <AssetField
        fieldId="thumbnail"
        fieldName="thumbnail"
        value={value()}
        persistedValue=""
        multiple={false}
        spaceId="default"
        generation={generation()}
        state={state}
        onChange={setValue}
      />
    ));

    fireEvent.change(screen.getByLabelText("Choose file"), {
      target: { files: [new File(["data"], "stale.txt")] },
    });
    await waitFor(() => expect(assetApi.upload).toHaveBeenCalled());

    setGeneration(2);
    resolveUpload?.(first);

    await waitFor(() => expect(state.pendingUploads()).toHaveLength(0));
    expect(value()).toBe("");
  });

  it("retries a failed upload without losing the local file", async () => {
    (assetApi.upload as ReturnType<typeof vi.fn>)
      .mockRejectedValueOnce(new Error("network failure"))
      .mockResolvedValueOnce(first);
    const [value, setValue] = createSignal("");

    render(() => (
      <AssetField
        fieldId="thumbnail"
        fieldName="thumbnail"
        value={value()}
        persistedValue=""
        multiple={false}
        spaceId="default"
        onChange={setValue}
      />
    ));

    fireEvent.change(screen.getByLabelText("Choose file"), {
      target: { files: [new File(["data"], "first.txt")] },
    });
    await waitFor(() =>
      expect(screen.getByText("network failure"))
        .toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry upload" }));
    await waitFor(() => expect(value()).toBe(serializeAssetReference(first)));
    expect(assetApi.upload).toHaveBeenCalledTimes(2);
  });

  it("keeps provisional local bytes across the Fields to Preview mount boundary", async () => {
    (assetApi.upload as ReturnType<typeof vi.fn>).mockResolvedValue(first);
    const [value, setValue] = createSignal("");
    const [mode, setMode] = createSignal<"fields" | "preview">("fields");
    const state = createAssetFieldState();

    render(() => (
      <Show
        when={mode() === "fields"}
        fallback={
          <AssetField
            fieldId="preview-thumbnail"
            fieldName="thumbnail"
            value={value()}
            persistedValue=""
            multiple={false}
            spaceId="default"
            formName="Media"
            entryId="entry-1"
            state={state}
            readOnly
            onChange={() => undefined}
          />
        }
      >
        <AssetField
          fieldId="thumbnail"
          fieldName="thumbnail"
          value={value()}
          persistedValue=""
          multiple={false}
          spaceId="default"
          state={state}
          onChange={setValue}
        />
      </Show>
    ));

    fireEvent.change(screen.getByLabelText("Choose file"), {
      target: { files: [new File(["data"], "first.txt")] },
    });
    await waitFor(() => expect(value()).toBe(serializeAssetReference(first)));
    setMode("preview");
    await waitFor(() =>
      expect(screen.getByText("first.txt"))
        .toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole("button", { name: "Open or download" }));
    expect(assetApi.read).not.toHaveBeenCalled();
  });

  it("applies queued list uploads to the latest draft after tab and list edits", async () => {
    const existing: AssetReference = {
      ...first,
      asset_id: "01900000-0000-7000-8000-000000000010",
      name: "existing.txt",
    };
    const resolvers: Array<(reference: AssetReference) => void> = [];
    (assetApi.upload as ReturnType<typeof vi.fn>).mockImplementation(
      () => new Promise<AssetReference>((resolve) => resolvers.push(resolve)),
    );
    const [value, setValue] = createSignal(
      serializeAssetReferenceList([existing]),
    );
    const [mode, setMode] = createSignal<"fields" | "preview">("fields");
    const state = createAssetFieldState();
    state.bindDraft({
      multiple: true,
      getValue: () => value(),
      setValue,
    });

    render(() => (
      <Show
        when={mode() === "fields"}
        fallback={
          <AssetField
            fieldId="preview-documents"
            fieldName="documents"
            value={value()}
            persistedValue={serializeAssetReferenceList([existing])}
            multiple
            spaceId="default"
            state={state}
            readOnly
            onChange={setValue}
          />
        }
      >
        <AssetField
          fieldId="documents"
          fieldName="documents"
          value={value()}
          persistedValue={serializeAssetReferenceList([existing])}
          multiple
          spaceId="default"
          state={state}
          onChange={setValue}
        />
      </Show>
    ));

    fireEvent.change(screen.getByLabelText("Choose file"), {
      target: {
        files: [
          new File(["one"], "queued-first.txt"),
          new File(["two"], "queued-second.txt"),
        ],
      },
    });
    await waitFor(() => expect(assetApi.upload).toHaveBeenCalledTimes(1));
    expect(state.pendingUploads()[0]?.replaceAssetId).toBeUndefined();
    resolvers.shift()?.(queuedFirst);
    await waitFor(() =>
      expect(value()).toBe(serializeAssetReferenceList([existing, queuedFirst]))
    );
    await waitFor(() => expect(assetApi.upload).toHaveBeenCalledTimes(2));

    // The second upload remains in flight while both conditional views are
    // remounted and the current list is changed.
    setMode("preview");
    await screen.findByText("queued-first.txt");
    setMode("fields");
    await screen.findByText("queued-first.txt");
    fireEvent.click(screen.getAllByRole("button", { name: / up$/ })[1]);
    fireEvent.click(screen.getAllByRole("button", { name: /^Remove$/ })[1]);
    expect(value()).toBe(serializeAssetReferenceList([queuedFirst]));

    resolvers.shift()?.(queuedSecond);
    await waitFor(() =>
      expect(value()).toBe(
        serializeAssetReferenceList([queuedFirst, queuedSecond]),
      )
    );
  });

  it("does not render SVG as an active image preview", async () => {
    const svg = {
      ...first,
      asset_id: "01900000-0000-7000-8000-000000000003",
      name: "diagram.svg",
      media_type: "image/svg+xml",
    };
    const state = createAssetFieldState();
    render(() => (
      <AssetField
        fieldId="raw-data"
        fieldName="raw_data"
        value={serializeAssetReference(svg)}
        persistedValue={serializeAssetReference(svg)}
        multiple={false}
        spaceId="default"
        entryId="entry-1"
        formName="Media"
        state={state}
        onChange={() => undefined}
      />
    ));
    state.setPreviewUrls(new Map([[svg.asset_id, "blob:svg"]]));
    await waitFor(() =>
      expect(screen.getByText("diagram.svg"))
        .toBeInTheDocument()
    );
    expect(screen.queryByRole("img", { name: "diagram.svg" })).toBeNull();
  });

  it("keeps logical metadata visible when persisted bytes are unavailable", async () => {
    (assetApi.read as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error("missing bytes"),
    );
    const encoded = serializeAssetReference(first);
    render(() => (
      <AssetField
        fieldId="document"
        fieldName="document"
        value={encoded}
        persistedValue={encoded}
        multiple={false}
        spaceId="default"
        formName="Contracts"
        entryId="entry-1"
        onChange={() => undefined}
      />
    ));
    fireEvent.click(screen.getByRole("button", { name: "Open or download" }));
    await waitFor(() =>
      expect(screen.getByText("File bytes unavailable; metadata preserved"))
        .toBeInTheDocument()
    );
    expect(screen.getByText("first.txt")).toBeInTheDocument();
  });

  it("ignores an authorized read completion after the initiating view unmounts", async () => {
    let resolveRead: ((blob: Blob) => void) | undefined;
    (assetApi.read as ReturnType<typeof vi.fn>).mockImplementation(
      () => new Promise<Blob>((resolve) => resolveRead = resolve),
    );
    const [mode, setMode] = createSignal<"fields" | "preview">("fields");
    const state = createAssetFieldState();
    const encoded = serializeAssetReference(first);
    const originalCreateObjectURL = URL.createObjectURL;
    const createObjectURL = vi.fn(() => "blob:should-not-be-created");
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: createObjectURL,
    });
    try {
      render(() => (
        <Show
          when={mode() === "fields"}
          fallback={
            <AssetField
              fieldId="preview-doc"
              fieldName="document"
              value={encoded}
              persistedValue={encoded}
              multiple={false}
              spaceId="default"
              formName="Contracts"
              entryId="entry-1"
              state={state}
              readOnly
              onChange={() => undefined}
            />
          }
        >
          <AssetField
            fieldId="document"
            fieldName="document"
            value={encoded}
            persistedValue={encoded}
            multiple={false}
            spaceId="default"
            formName="Contracts"
            entryId="entry-1"
            state={state}
            onChange={() => undefined}
          />
        </Show>
      ));
      fireEvent.click(screen.getByRole("button", { name: "Open or download" }));
      await waitFor(() => expect(assetApi.read).toHaveBeenCalled());
      setMode("preview");
      resolveRead?.(new Blob(["data"], { type: "text/plain" }));
      await waitFor(() => expect(state.readingIds().size).toBe(0));
      expect(createObjectURL).not.toHaveBeenCalled();
    } finally {
      Object.defineProperty(URL, "createObjectURL", {
        configurable: true,
        value: originalCreateObjectURL,
      });
    }
  });
});
