import { createEffect, createMemo, For, onCleanup, Show } from "solid-js";
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
  let observedPersistedIds: Set<string> | undefined;

  createEffect(() => {
    // Generation reset is owned by the Entry draft. It is deliberately not
    // tied to this component's mount lifetime, because Fields and Preview
    // are separate conditional views of the same draft state.
    state.resetForGeneration(props.generation ?? 0);
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

  const readReference = async (reference: AssetReference) => {
    const activeGeneration = props.generation ?? 0;
    const controller = new AbortController();
    state.readControllers.set(reference.asset_id, controller);
    setReadingIds((ids) => new Set([...ids, reference.asset_id]));
    try {
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
          controller.signal,
        );
      }
      // A tab switch may unmount this view while the authorized read is in
      // flight. Do not create object URLs or trigger a download from a dead
      // view; a later mount can retry using the shared draft state.
      if (
        disposed || controller.signal.aborted ||
        !state.isActive(activeGeneration)
      ) return;
      const url = URL.createObjectURL(blob);
      setPreviewUrls((urls) => {
        const next = new Map(urls);
        const previous = next.get(reference.asset_id);
        if (previous) URL.revokeObjectURL(previous);
        next.set(reference.asset_id, url);
        return next;
      });
      setUnavailableIds((ids) => {
        const next = new Set(ids);
        next.delete(reference.asset_id);
        return next;
      });
      const link = document.createElement("a");
      link.href = url;
      link.download = reference.name;
      link.click();
    } catch (error) {
      if (!isAbortError(error) && state.isActive(activeGeneration)) {
        setUnavailableIds((ids) => new Set([...ids, reference.asset_id]));
      }
    } finally {
      if (state.isActive(activeGeneration)) {
        state.readControllers.delete(reference.asset_id);
        setReadingIds((ids) => {
          const next = new Set(ids);
          next.delete(reference.asset_id);
          return next;
        });
      }
    }
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
          <div class="ui-card ui-asset-item">
            <Show
              when={previewUrls().get(reference.asset_id) &&
                reference.media_type.startsWith("image/") &&
                reference.media_type !== "image/svg+xml"}
            >
              <img
                class="ui-asset-preview"
                src={previewUrls().get(reference.asset_id)}
                alt={reference.name}
              />
            </Show>
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
              <button
                type="button"
                class="ui-button ui-button-secondary ui-button-sm"
                onClick={() => void readReference(reference)}
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
