import { createEffect, createMemo, For, onCleanup, Show } from "solid-js";
import { AssetPreview } from "./AssetPreview";
import { assetApi } from "~/lib/ugoite-client";
import { locale, t } from "~/lib/i18n";
import {
  type AssetDraftBinding,
  type AssetFieldState,
  type AssetUploadMessages,
  createAssetFieldState,
  type PendingAssetUpload,
} from "~/lib/asset-field-state";
import {
  formatAssetSize,
  parseAssetReference,
  parseAssetReferenceList,
} from "~/lib/asset-reference";
import { previewMediaType, resolvePreviewKind } from "~/lib/asset-preview";
import type { AssetReference } from "~/lib/types";

type PendingUpload = PendingAssetUpload;

export interface AssetFieldProps {
  fieldId: string;
  fieldName: string;
  value: string;
  persistedValue: string;
  multiple: boolean;
  spaceId: string;
  formName?: string;
  entryId?: string;
  readOnly?: boolean;
  generation?: number;
  state?: AssetFieldState;
  invalid?: boolean;
  describedBy?: string;
  onChange: (value: string) => void;
}

const createUploadId = () =>
  `upload-${Date.now()}-${Math.random().toString(36).slice(2)}`;

const isAbortError = (error: unknown) =>
  error instanceof Error && error.name === "AbortError";

/** Form-owned asset_reference/list<asset_reference> editor. */
export function AssetField(props: AssetFieldProps) {
  const state = props.state ?? createAssetFieldState();
  const ownsState = !props.state;
  const pendingUploads = state.pendingUploads;
  const previewUrls = state.previewUrls;
  const setPreviewUrls = state.setPreviewUrls;
  const previewBlobs = state.previewBlobs;
  const setPreviewBlobs = state.setPreviewBlobs;
  const previewSignatures = state.previewSignatures;
  const setPreviewSignatures = state.setPreviewSignatures;
  const unavailableIds = state.unavailableIds;
  const setUnavailableIds = state.setUnavailableIds;
  const readingIds = state.readingIds;
  const setReadingIds = state.setReadingIds;
  const localFiles = state.localFiles;
  let disposed = false;

  if (!state.hasDraftBinding()) {
    const binding: AssetDraftBinding = {
      multiple: props.multiple,
      getValue: () => props.value,
      setValue: props.onChange,
    };
    state.bindDraft(binding);
  }

  const parsedValue = createMemo(() => {
    if (props.multiple) {
      const references = parseAssetReferenceList(props.value);
      return {
        references: references ?? [],
        invalid: props.value.trim().length > 0 && references === null,
      };
    }
    const reference = parseAssetReference(props.value);
    return {
      references: reference ? [reference] : [],
      invalid: props.value.trim().length > 0 && reference === null,
    };
  });

  const persistedIds = createMemo(() => {
    const persisted = props.multiple
      ? parseAssetReferenceList(props.persistedValue) ?? []
      : [parseAssetReference(props.persistedValue)].filter(
        (value): value is AssetReference => value !== null,
      );
    return new Set(persisted.map((reference) => reference.asset_id));
  });
  const previewSignature = (reference: AssetReference) =>
    `${reference.name}\u0000${previewMediaType(reference)}\u0000${
      resolvePreviewKind(reference)
    }`;
  let observedPersistedIds: Set<string> | undefined;

  createEffect(() => {
    // Generation reset is owned by the Entry draft. It is deliberately not
    // tied to this component's mount lifetime, because Fields and Preview
    // are separate conditional views of the same draft state.
    state.resetForGeneration(props.generation ?? 0);
  });

  createEffect(() => {
    const referencesById = new Map(
      parsedValue().references.map((
        reference,
      ) => [reference.asset_id, reference]),
    );
    for (const [assetId, signature] of previewSignatures()) {
      const reference = referencesById.get(assetId);
      if (!reference || previewSignature(reference) !== signature) {
        state.invalidatePreview(assetId);
      }
    }
  });

  createEffect(() => {
    const persisted = persistedIds();
    if (!observedPersistedIds) {
      observedPersistedIds = new Set(persisted);
      return;
    }
    setUnavailableIds((ids) => {
      const next = new Set(ids);
      for (const id of persisted) {
        if (!observedPersistedIds?.has(id)) next.delete(id);
      }
      return next;
    });
    observedPersistedIds = new Set(persisted);
  });

  onCleanup(() => {
    disposed = true;
    // A shared state survives a Fields/Preview tab switch. Standalone fields
    // still own and clean up their state when no Entry draft provided one.
    if (ownsState) state.dispose();
  });

  const uploadMessages = (): AssetUploadMessages => ({
    invalid: t("assetField.error.invalid"),
    duplicate: t("assetField.error.duplicate"),
    replacedItemMissing: t("assetField.error.replacedItemMissing"),
    uploadFailed: t("assetField.error.uploadFailed"),
  });

  const upload = (file: File, signal: AbortSignal) =>
    assetApi.upload(props.spaceId, file, file.name, signal);

  const chooseFiles = (files: File[], replaceAssetId?: string) => {
    const selected = replaceAssetId || !props.multiple
      ? files.slice(0, 1)
      : files;
    if (selected.length === 0) return;
    const targetAssetId = replaceAssetId ||
      (props.multiple ? undefined : parsedValue().references[0]?.asset_id);
    for (const file of selected) {
      state.enqueueUpload(
        {
          id: createUploadId(),
          file,
          replaceAssetId: targetAssetId,
          generation: props.generation ?? 0,
          status: "local",
          controller: new AbortController(),
        },
        upload,
        uploadMessages(),
      );
    }
  };

  const handleDrop = (event: DragEvent) => {
    event.preventDefault();
    if (props.readOnly) return;
    chooseFiles(Array.from(event.dataTransfer?.files ?? []));
  };

  const retry = (item: PendingUpload) => {
    state.retryUpload(item, upload, uploadMessages());
  };

  const cancel = (item: PendingUpload) => {
    state.cancelUpload(item);
  };

  const removeReference = (index: number) => {
    state.removeReference(index);
  };

  const moveReference = (index: number, delta: -1 | 1) => {
    state.moveReference(index, delta);
  };

  const loadReference = async (
    reference: AssetReference,
    signal: AbortSignal,
    purpose: "preview" | "download",
  ) => {
    const isPersisted = persistedIds().has(reference.asset_id);
    const localFile = localFiles().get(reference.asset_id);
    let blob: Blob;
    if (!isPersisted && localFile) {
      // A provisional upload is not yet readable through the authorized
      // Entry endpoint. Keep the local File usable without pretending it is
      // already attached to the Entry.
      blob = localFile;
    } else {
      const formName = props.formName?.trim();
      const entryId = props.entryId?.trim();
      if (!formName || !entryId) throw new Error("Asset context unavailable");
      blob = await assetApi.read(
        props.spaceId,
        reference.asset_id,
        formName,
        entryId,
        signal,
      );
    }
    // get_asset returns application/octet-stream by design. Form-owned
    // metadata supplies the logical type needed by browser-native previews;
    // downloads stay inert even when the logical type is active markup.
    return new Blob([blob], {
      type: purpose === "preview"
        ? previewMediaType(reference)
        : "application/octet-stream",
    });
  };

  const readReference = async (
    reference: AssetReference,
    purpose: "preview" | "download",
    action: (blob: Blob) => void,
  ) => {
    const activeGeneration = props.generation ?? 0;
    const controller = new AbortController();
    state.readControllers.set(reference.asset_id, controller);
    setReadingIds((ids) => new Set([...ids, reference.asset_id]));
    try {
      // A tab switch may unmount this view while the authorized read is in
      // flight. Do not create object URLs or trigger a download from a dead
      // view; a later mount can retry using the shared draft state.
      const blob = await loadReference(reference, controller.signal, purpose);
      if (
        disposed || controller.signal.aborted ||
        !state.isActive(activeGeneration)
      ) return;
      action(blob);
      setUnavailableIds((ids) => {
        const next = new Set(ids);
        next.delete(reference.asset_id);
        return next;
      });
    } catch (error) {
      if (!isAbortError(error) && state.isActive(activeGeneration)) {
        setUnavailableIds((ids) => new Set([...ids, reference.asset_id]));
      }
    } finally {
      if (state.isActive(activeGeneration)) {
        if (state.readControllers.get(reference.asset_id) === controller) {
          state.readControllers.delete(reference.asset_id);
        }
        setReadingIds((ids) => {
          if (state.readControllers.has(reference.asset_id)) return ids;
          const next = new Set(ids);
          next.delete(reference.asset_id);
          return next;
        });
      }
    }
  };

  const previewReference = (reference: AssetReference) => {
    const signature = previewSignature(reference);
    if (
      previewUrls().has(reference.asset_id) &&
      previewBlobs().has(reference.asset_id) &&
      previewSignatures().get(reference.asset_id) === signature
    ) return;
    state.invalidatePreview(reference.asset_id);
    setPreviewSignatures((signatures) => {
      const next = new Map(signatures);
      next.set(reference.asset_id, signature);
      return next;
    });
    void readReference(reference, "preview", (blob) => {
      const url = URL.createObjectURL(blob);
      setPreviewBlobs((blobs) => {
        const next = new Map(blobs);
        next.set(reference.asset_id, blob);
        return next;
      });
      setPreviewUrls((urls) => {
        const next = new Map(urls);
        const previous = next.get(reference.asset_id);
        if (previous) URL.revokeObjectURL(previous);
        next.set(reference.asset_id, url);
        return next;
      });
    });
  };

  const downloadReference = (reference: AssetReference) => {
    const existingUrl = previewSignatures().get(reference.asset_id) ===
        previewSignature(reference)
      ? previewUrls().get(reference.asset_id)
      : undefined;
    const cachedBlob = previewBlobs().get(reference.asset_id);
    if (existingUrl && cachedBlob) {
      const url = URL.createObjectURL(
        new Blob([cachedBlob], { type: "application/octet-stream" }),
      );
      const link = document.createElement("a");
      link.href = url;
      link.download = reference.name;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      return;
    }
    void readReference(reference, "download", (blob) => {
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = reference.name;
      link.click();
      // Keep the URL alive long enough for browsers to start asynchronous
      // download navigation, including Safari and Firefox.
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    });
  };

  const previewFor = (assetId: string) => {
    const reference = parsedValue().references.find((candidate) =>
      candidate.asset_id === assetId
    );
    if (
      !reference ||
      previewSignatures().get(assetId) !== previewSignature(reference)
    ) return undefined;
    const url = previewUrls().get(assetId);
    const blob = previewBlobs().get(assetId);
    return url && blob ? { url, blob } : undefined;
  };

  const statusText = (reference: AssetReference) => {
    if (unavailableIds().has(reference.asset_id)) {
      return t("assetField.status.unavailable");
    }
    return persistedIds().has(reference.asset_id)
      ? t("assetField.status.persisted")
      : t("assetField.status.uploaded");
  };

  return (
    <div
      class="ui-asset-field ui-stack-sm"
      data-field-name={props.fieldName}
      onDragOver={(event) => event.preventDefault()}
      onDrop={handleDrop}
    >
      <p class="text-xs ui-muted">
        {t(
          props.multiple
            ? "assetField.description.list"
            : "assetField.description.scalar",
        )}
      </p>

      <Show when={parsedValue().invalid}>
        <p class="ui-alert ui-alert-error text-sm" role="alert">
          {t("assetField.error.invalid")}
        </p>
      </Show>

      <For each={parsedValue().references}>
        {(reference, index) => (
          <div class="ui-card ui-asset-item ui-asset-item-with-preview">
            <div class="ui-asset-item-header">
              <div class="min-w-0 flex-1">
                <p class="truncate font-medium">{reference.name}</p>
                <p class="text-xs ui-muted">
                  {reference.media_type} · {formatAssetSize(
                    reference.size_bytes,
                    locale() === "ja" ? "ja-JP" : "en-US",
                  )}
                </p>
                <p class="text-xs ui-muted" role="status">
                  {statusText(reference)}
                </p>
              </div>
              <div class="flex flex-wrap items-center justify-end gap-2">
                <Show when={resolvePreviewKind(reference) !== "unsupported"}>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary ui-button-sm"
                    onClick={() => previewReference(reference)}
                    disabled={readingIds().has(reference.asset_id) ||
                      (!localFiles().has(reference.asset_id) &&
                        (!persistedIds().has(reference.asset_id) ||
                          !props.formName?.trim() || !props.entryId?.trim()))}
                  >
                    {readingIds().has(reference.asset_id)
                      ? t("assetField.status.reading")
                      : t("assetField.action.preview")}
                  </button>
                </Show>
                <button
                  type="button"
                  class="ui-button ui-button-secondary ui-button-sm"
                  onClick={() => downloadReference(reference)}
                  disabled={readingIds().has(reference.asset_id) ||
                    (!localFiles().has(reference.asset_id) &&
                      (!persistedIds().has(reference.asset_id) ||
                        !props.formName?.trim() || !props.entryId?.trim()))}
                >
                  {readingIds().has(reference.asset_id)
                    ? t("assetField.status.reading")
                    : t("assetField.action.download")}
                </button>
                <Show when={!props.readOnly}>
                  <label class="ui-button ui-button-secondary ui-button-sm">
                    {t("assetField.action.replace")}
                    <input
                      type="file"
                      class="ui-sr-only"
                      aria-label={t("assetField.action.replace")}
                      onChange={(event) => {
                        const input = event.currentTarget;
                        chooseFiles(
                          Array.from(input.files ?? []),
                          reference.asset_id,
                        );
                        input.value = "";
                      }}
                    />
                  </label>
                </Show>
                <Show when={!props.readOnly && props.multiple}>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary ui-button-sm"
                    onClick={() => moveReference(index(), -1)}
                    disabled={index() === 0}
                    aria-label={t("assetField.action.moveUp", {
                      name: reference.name,
                    })}
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary ui-button-sm"
                    onClick={() => moveReference(index(), 1)}
                    disabled={index() === parsedValue().references.length - 1}
                    aria-label={t("assetField.action.moveDown", {
                      name: reference.name,
                    })}
                  >
                    ↓
                  </button>
                </Show>
                <Show when={!props.readOnly}>
                  <button
                    type="button"
                    class="ui-button ui-button-danger ui-button-sm"
                    onClick={() => removeReference(index())}
                    aria-label={t("assetField.action.remove", {
                      name: reference.name,
                    })}
                  >
                    {t("assetField.action.remove")}
                  </button>
                </Show>
              </div>
            </div>
            <Show when={previewFor(reference.asset_id)}>
              {(preview) => (
                <AssetPreview
                  reference={reference}
                  blob={preview().blob}
                  url={preview().url}
                />
              )}
            </Show>
          </div>
        )}
      </For>

      <Show when={!props.readOnly}>
        <For each={pendingUploads()}>
          {(item) => (
            <div
              class="ui-card ui-asset-item"
              aria-busy={item.status === "uploading"}
            >
              <div class="min-w-0 flex-1">
                <p class="truncate font-medium">{item.file.name}</p>
                <p class="text-xs ui-muted">
                  {formatAssetSize(
                    item.file.size,
                    locale() === "ja" ? "ja-JP" : "en-US",
                  )}
                </p>
                <p
                  class={item.status === "failed"
                    ? "text-xs ui-text-danger"
                    : "text-xs ui-muted"}
                  role="status"
                  aria-live="polite"
                >
                  {item.status === "local"
                    ? t("assetField.status.local")
                    : item.status === "uploading"
                    ? t("assetField.status.uploading")
                    : item.error ?? t("assetField.error.uploadFailed")}
                </p>
              </div>
              <div class="flex gap-2">
                <Show when={item.status === "failed"}>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary ui-button-sm"
                    onClick={() => retry(item)}
                  >
                    {t("assetField.action.retry")}
                  </button>
                </Show>
                <button
                  type="button"
                  class="ui-button ui-button-secondary ui-button-sm"
                  onClick={() => cancel(item)}
                >
                  {t("assetField.action.cancel")}
                </button>
              </div>
            </div>
          )}
        </For>

        <label
          class="ui-button ui-button-secondary inline-flex w-fit cursor-pointer"
          for={`${props.fieldId}-file`}
        >
          {t("assetField.action.choose")}
          <input
            id={`${props.fieldId}-file`}
            type="file"
            class="ui-sr-only"
            multiple={props.multiple}
            aria-label={t("assetField.action.choose")}
            aria-invalid={props.invalid ? "true" : undefined}
            aria-describedby={props.invalid ? props.describedBy : undefined}
            onChange={(event) => {
              const input = event.currentTarget;
              chooseFiles(Array.from(input.files ?? []));
              input.value = "";
            }}
          />
        </label>
        <p class="text-xs ui-muted">{t("assetField.drop")}</p>
      </Show>
    </div>
  );
}
