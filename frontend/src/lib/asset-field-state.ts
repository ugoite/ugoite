import {
  createSignal,
  type Accessor,
  type Setter,
} from "solid-js";

export type PendingAssetUpload = {
  id: string;
  file: File;
  replaceAssetId?: string;
  generation: number;
  status: "local" | "uploading" | "failed";
  error?: string;
  controller: AbortController;
};

/**
 * Non-serializable state owned by one Entry draft and shared by every view of
 * an asset field. The Entry pane, rather than a conditionally mounted view,
 * owns the lifetime of this state.
 */
export interface AssetFieldState {
  pendingUploads: Accessor<PendingAssetUpload[]>;
  setPendingUploads: Setter<PendingAssetUpload[]>;
  previewUrls: Accessor<Map<string, string>>;
  setPreviewUrls: Setter<Map<string, string>>;
  unavailableIds: Accessor<Set<string>>;
  setUnavailableIds: Setter<Set<string>>;
  readingIds: Accessor<Set<string>>;
  setReadingIds: Setter<Set<string>>;
  localFiles: Accessor<Map<string, File>>;
  setLocalFiles: Setter<Map<string, File>>;
  readControllers: Map<string, AbortController>;
  uploadQueue: Promise<void>;
  setUploadQueue: (queue: Promise<void>) => void;
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

  return {
    pendingUploads,
    setPendingUploads,
    previewUrls,
    setPreviewUrls,
    unavailableIds,
    setUnavailableIds,
    readingIds,
    setReadingIds,
    localFiles,
    setLocalFiles,
    readControllers,
    get uploadQueue() {
      return uploadQueue;
    },
    setUploadQueue: (queue) => {
      uploadQueue = queue;
    },
    resetForGeneration,
    dispose: () => resetForGeneration((generation ?? 0) + 1),
    isActive: (activeGeneration) => generation === activeGeneration,
  };
}
