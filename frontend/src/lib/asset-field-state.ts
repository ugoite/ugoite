import { type Accessor, createSignal, type Setter } from "solid-js";
import {
  isAssetReference,
  parseAssetReference,
  parseAssetReferenceList,
  serializeAssetReference,
  serializeAssetReferenceList,
} from "./asset-reference";
import type { AssetReference } from "./types";

export type PendingAssetUpload = {
  id: string;
  file: File;
  replaceAssetId?: string;
  generation: number;
  status: "local" | "uploading" | "failed";
  error?: string;
  controller: AbortController;
};

export type AssetDraftBinding = {
  multiple: boolean;
  getValue: () => string;
  setValue: (value: string) => void;
};

export type AssetUploadMessages = {
  invalid: string;
  duplicate: string;
  replacedItemMissing: string;
  uploadFailed: string;
};

export type AssetUpload = (
  file: File,
  signal: AbortSignal,
) => Promise<AssetReference>;

/**
 * Non-serializable state owned by one Entry draft and shared by every view of
 * an asset field. The Entry pane, rather than a conditionally mounted view,
 * owns the lifetime of this state.
 */
export interface AssetFieldState {
  pendingUploads: Accessor<PendingAssetUpload[]>;
  previewUrls: Accessor<Map<string, string>>;
  setPreviewUrls: Setter<Map<string, string>>;
  unavailableIds: Accessor<Set<string>>;
  setUnavailableIds: Setter<Set<string>>;
  readingIds: Accessor<Set<string>>;
  setReadingIds: Setter<Set<string>>;
  localFiles: Accessor<Map<string, File>>;
  setLocalFiles: Setter<Map<string, File>>;
  readControllers: Map<string, AbortController>;
  bindDraft: (binding: AssetDraftBinding) => void;
  hasDraftBinding: () => boolean;
  enqueueUpload: (
    item: PendingAssetUpload,
    upload: AssetUpload,
    messages: AssetUploadMessages,
  ) => void;
  retryUpload: (
    item: PendingAssetUpload,
    upload: AssetUpload,
    messages: AssetUploadMessages,
  ) => void;
  cancelUpload: (item: PendingAssetUpload) => void;
  removeReference: (index: number) => void;
  moveReference: (index: number, delta: -1 | 1) => void;
  resetForGeneration: (generation: number) => void;
  dispose: () => void;
  isActive: (generation: number) => boolean;
}

export function createAssetFieldState(): AssetFieldState {
  const [pendingUploads, setPendingUploads] = createSignal<
    PendingAssetUpload[]
  >([]);
  const [previewUrls, setPreviewUrls] = createSignal<Map<string, string>>(
    new Map(),
  );
  const [unavailableIds, setUnavailableIds] = createSignal<Set<string>>(
    new Set(),
  );
  const [readingIds, setReadingIds] = createSignal<Set<string>>(new Set());
  const [localFiles, setLocalFiles] = createSignal<Map<string, File>>(
    new Map(),
  );
  const readControllers = new Map<string, AbortController>();
  let uploadQueue = Promise.resolve();
  let generation: number | undefined;
  let draftBinding: AssetDraftBinding | undefined;

  const revokePreviewUrls = () => {
    for (const url of previewUrls().values()) URL.revokeObjectURL(url);
  };

  const resetForGeneration = (nextGeneration: number) => {
    if (generation === nextGeneration) return;
    generation = nextGeneration;
    for (const item of pendingUploads()) item.controller.abort();
    for (const controller of readControllers.values()) controller.abort();
    readControllers.clear();
    // Do not make a new draft wait for an aborted upload chain from the old
    // generation. Its callbacks are still fenced by isActive().
    uploadQueue = Promise.resolve();
    setPendingUploads([]);
    setReadingIds(new Set());
    setUnavailableIds(new Set());
    revokePreviewUrls();
    setPreviewUrls(new Map());
    setLocalFiles(new Map());
  };

  const bindDraft = (binding: AssetDraftBinding) => {
    draftBinding = binding;
  };

  const pendingById = (id: string) =>
    pendingUploads().find((item) => item.id === id);

  const isActiveUpload = (item: PendingAssetUpload) =>
    !item.controller.signal.aborted && isActive(item.generation);

  const currentReferences = () => {
    const binding = draftBinding;
    if (!binding) return [];
    if (binding.multiple) {
      return parseAssetReferenceList(binding.getValue()) ?? [];
    }
    const reference = parseAssetReference(binding.getValue());
    return reference ? [reference] : [];
  };

  const setReferences = (references: AssetReference[]) => {
    const binding = draftBinding;
    if (!binding) return;
    binding.setValue(
      binding.multiple
        ? serializeAssetReferenceList(references)
        : references[0]
        ? serializeAssetReference(references[0])
        : "",
    );
  };

  const removePendingForAsset = (assetId: string) => {
    setPendingUploads((items) => {
      for (const item of items) {
        if (item.replaceAssetId === assetId) item.controller.abort();
      }
      return items.filter((item) => item.replaceAssetId !== assetId);
    });
  };

  const removeReference = (index: number) => {
    const current = currentReferences();
    const removed = current[index];
    if (!removed) return;
    removePendingForAsset(removed.asset_id);
    setLocalFiles((files) => {
      const next = new Map(files);
      next.delete(removed.asset_id);
      return next;
    });
    setPreviewUrls((urls) => {
      const next = new Map(urls);
      const preview = next.get(removed.asset_id);
      if (preview) URL.revokeObjectURL(preview);
      next.delete(removed.asset_id);
      return next;
    });
    setUnavailableIds((ids) => {
      const next = new Set(ids);
      next.delete(removed.asset_id);
      return next;
    });
    setReferences(current.filter((_, itemIndex) => itemIndex !== index));
  };

  const moveReference = (index: number, delta: -1 | 1) => {
    const current = currentReferences();
    const target = index + delta;
    if (target < 0 || target >= current.length) return;
    const next = current.slice();
    [next[index], next[target]] = [next[target], next[index]];
    setReferences(next);
  };

  const runUpload = async (
    id: string,
    upload: AssetUpload,
    messages: AssetUploadMessages,
  ) => {
    const item = pendingById(id);
    if (!item) return;
    if (!isActiveUpload(item)) {
      setPendingUploads((items) =>
        items.filter((candidate) => candidate.id !== id)
      );
      return;
    }
    setPendingUploads((items) =>
      items.map((candidate) =>
        candidate.id === id ? { ...candidate, status: "uploading" } : candidate
      )
    );
    try {
      const reference = await upload(item.file, item.controller.signal);
      if (!isAssetReference(reference)) throw new Error(messages.invalid);
      if (!isActiveUpload(item)) {
        setPendingUploads((items) =>
          items.filter((candidate) => candidate.id !== id)
        );
        return;
      }

      // Resolve against the current Entry draft, not a value captured by a
      // conditionally mounted view. Reorder/remove operations made while a
      // queued upload is running are therefore preserved.
      const current = currentReferences();
      const targetIndex = item.replaceAssetId === undefined
        ? -1
        : current.findIndex((candidate) =>
          candidate.asset_id === item.replaceAssetId
        );
      const duplicate = current.some((candidate, candidateIndex) =>
        candidate.asset_id === reference.asset_id &&
        candidateIndex !== targetIndex
      );
      if (
        (item.replaceAssetId === undefined && !draftBinding?.multiple &&
          current.length > 0) ||
        (item.replaceAssetId !== undefined && targetIndex < 0) ||
        duplicate
      ) {
        setPendingUploads((items) =>
          items.map((candidate) =>
            candidate.id === id
              ? {
                ...candidate,
                status: "failed",
                error: item.replaceAssetId !== undefined && targetIndex < 0
                  ? messages.replacedItemMissing
                  : messages.duplicate,
              }
              : candidate
          )
        );
        return;
      }

      const next = current.slice();
      if (targetIndex >= 0) next[targetIndex] = reference;
      else next.push(reference);
      setReferences(next);

      if (item.replaceAssetId) {
        setPreviewUrls((urls) => {
          const nextUrls = new Map(urls);
          const preview = nextUrls.get(item.replaceAssetId!);
          if (preview) URL.revokeObjectURL(preview);
          nextUrls.delete(item.replaceAssetId!);
          return nextUrls;
        });
        setUnavailableIds((ids) => {
          const nextIds = new Set(ids);
          nextIds.delete(item.replaceAssetId!);
          return nextIds;
        });
      }
      setLocalFiles((files) => {
        const nextFiles = new Map(files);
        if (item.replaceAssetId) nextFiles.delete(item.replaceAssetId);
        nextFiles.set(reference.asset_id, item.file);
        return nextFiles;
      });
      setPendingUploads((items) =>
        items.filter((candidate) => candidate.id !== id)
      );
    } catch (error) {
      if (
        !isActiveUpload(item) ||
        error instanceof Error && error.name === "AbortError"
      ) {
        setPendingUploads((items) =>
          items.filter((candidate) => candidate.id !== id)
        );
        return;
      }
      setPendingUploads((items) =>
        items.map((candidate) =>
          candidate.id === id
            ? {
              ...candidate,
              status: "failed",
              error: error instanceof Error
                ? error.message
                : messages.uploadFailed,
            }
            : candidate
        )
      );
    }
  };

  const enqueueUpload = (
    item: PendingAssetUpload,
    upload: AssetUpload,
    messages: AssetUploadMessages,
  ) => {
    setPendingUploads((items) => [...items, item]);
    const next = uploadQueue.then(() => runUpload(item.id, upload, messages));
    uploadQueue = next;
  };

  const retryUpload = (
    item: PendingAssetUpload,
    upload: AssetUpload,
    messages: AssetUploadMessages,
  ) => {
    const controller = new AbortController();
    setPendingUploads((items) =>
      items.map((candidate) =>
        candidate.id === item.id
          ? { ...candidate, controller, status: "local", error: undefined }
          : candidate
      )
    );
    const next = uploadQueue.then(() => runUpload(item.id, upload, messages));
    uploadQueue = next;
  };

  const cancelUpload = (item: PendingAssetUpload) => {
    item.controller.abort();
    setPendingUploads((items) =>
      items.filter((candidate) => candidate.id !== item.id)
    );
  };

  const isActive = (activeGeneration: number) =>
    generation === activeGeneration;

  return {
    pendingUploads,
    previewUrls,
    setPreviewUrls,
    unavailableIds,
    setUnavailableIds,
    readingIds,
    setReadingIds,
    localFiles,
    setLocalFiles,
    readControllers,
    bindDraft,
    hasDraftBinding: () => draftBinding !== undefined,
    enqueueUpload,
    retryUpload,
    cancelUpload,
    removeReference,
    moveReference,
    resetForGeneration,
    dispose: () => resetForGeneration((generation ?? 0) + 1),
    isActive,
  };
}
