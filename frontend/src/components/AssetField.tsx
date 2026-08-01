import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { assetApi } from "~/lib/ugoite-client";
import { locale, t } from "~/lib/i18n";
import {
  formatAssetSize,
  parseAssetReference,
  parseAssetReferenceList,
  serializeAssetReference,
  serializeAssetReferenceList,
} from "~/lib/asset-reference";
import type { AssetReference } from "~/lib/types";

type PendingUpload = {
  id: string;
  file: File;
  replaceIndex?: number;
  status: "local" | "uploading" | "failed";
  error?: string;
  controller: AbortController;
};

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
  onChange: (value: string) => void;
  onPendingChange?: (pending: boolean) => void;
}

const createUploadId = () =>
  `upload-${Date.now()}-${Math.random().toString(36).slice(2)}`;

const isAbortError = (error: unknown) =>
  error instanceof Error && error.name === "AbortError";

/** Form-owned asset_reference/list<asset_reference> editor. */
export function AssetField(props: AssetFieldProps) {
  const [pendingUploads, setPendingUploads] = createSignal<PendingUpload[]>(
    [],
  );
  const [previewUrls, setPreviewUrls] = createSignal<Map<string, string>>(
    new Map(),
  );
  const [unavailableIds, setUnavailableIds] = createSignal<Set<string>>(
    new Set(),
  );
  const [readingIds, setReadingIds] = createSignal<Set<string>>(new Set());
  let uploadQueue = Promise.resolve();
  let chooseFileInput: HTMLInputElement | undefined;
  let replaceFileInput: HTMLInputElement | undefined;

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

  createEffect(() => props.onPendingChange?.(pendingUploads().length > 0));

  onCleanup(() => {
    for (const item of pendingUploads()) item.controller.abort();
    for (const url of previewUrls().values()) URL.revokeObjectURL(url);
  });

  const pendingById = (id: string) =>
    pendingUploads().find((item) => item.id === id);

  const setPending = (
    id: string,
    update: (item: PendingUpload) => PendingUpload,
  ) => {
    setPendingUploads((items) =>
      items.map((item) => item.id === id ? update(item) : item)
    );
  };

  const removePending = (id: string) => {
    setPendingUploads((items) => items.filter((item) => item.id !== id));
  };

  const addReference = (reference: AssetReference) => {
    const current = parsedValue().references;
    if (current.some((item) => item.asset_id === reference.asset_id)) {
      return false;
    }
    if (props.multiple) {
      props.onChange(serializeAssetReferenceList([...current, reference]));
    } else {
      props.onChange(serializeAssetReference(reference));
    }
    return true;
  };

  const replaceReference = (reference: AssetReference, index: number) => {
    const current = parsedValue().references;
    if (
      current.some((item, itemIndex) =>
        itemIndex !== index && item.asset_id === reference.asset_id
      )
    ) {
      return false;
    }
    if (props.multiple) {
      const next = current.slice();
      next[index] = reference;
      props.onChange(serializeAssetReferenceList(next));
    } else {
      props.onChange(serializeAssetReference(reference));
    }
    return true;
  };

  const runUpload = async (id: string) => {
    const item = pendingById(id);
    if (!item) return;
    if (item.controller.signal.aborted) {
      removePending(id);
      return;
    }
    setPending(id, (current) => ({ ...current, status: "uploading" }));
    try {
      const reference = await assetApi.upload(
        props.spaceId,
        item.file,
        item.file.name,
        item.controller.signal,
      );
      if (item.controller.signal.aborted) {
        removePending(id);
        return;
      }
      const accepted = item.replaceIndex === undefined
        ? addReference(reference)
        : replaceReference(reference, item.replaceIndex);
      if (!accepted) {
        setPending(id, (current) => ({
          ...current,
          status: "failed",
          error: t("assetField.error.duplicate"),
        }));
        return;
      }
      removePending(id);
    } catch (error) {
      if (isAbortError(error)) {
        removePending(id);
        return;
      }
      setPending(id, (current) => ({
        ...current,
        status: "failed",
        error: error instanceof Error
          ? error.message
          : t("assetField.error.uploadFailed"),
      }));
    }
  };

  const enqueueUpload = (item: PendingUpload) => {
    setPendingUploads((items) => [...items, item]);
    uploadQueue = uploadQueue.then(() => runUpload(item.id));
  };

  const chooseFiles = (files: File[]) => {
    const selected = props.multiple ? files : files.slice(0, 1);
    if (selected.length === 0) return;
    const replaceIndex = props.multiple
      ? undefined
      : parsedValue().references.length > 0
      ? 0
      : undefined;
    for (const file of selected) {
      enqueueUpload({
        id: createUploadId(),
        file,
        replaceIndex,
        status: "local",
        controller: new AbortController(),
      });
    }
    if (chooseFileInput) chooseFileInput.value = "";
    if (replaceFileInput) replaceFileInput.value = "";
  };

  const handleDrop = (event: DragEvent) => {
    event.preventDefault();
    if (props.readOnly) return;
    chooseFiles(Array.from(event.dataTransfer?.files ?? []));
  };

  const retry = (item: PendingUpload) => {
    const controller = new AbortController();
    setPending(item.id, (current) => ({
      ...current,
      controller,
      status: "local",
      error: undefined,
    }));
    uploadQueue = uploadQueue.then(() => runUpload(item.id));
  };

  const cancel = (item: PendingUpload) => {
    item.controller.abort();
    removePending(item.id);
  };

  const removeReference = (index: number) => {
    const current = parsedValue().references;
    if (props.multiple) {
      props.onChange(
        serializeAssetReferenceList(
          current.filter((_, itemIndex) => itemIndex !== index),
        ),
      );
    } else {
      props.onChange("");
    }
  };

  const moveReference = (index: number, delta: -1 | 1) => {
    const current = parsedValue().references;
    const target = index + delta;
    if (target < 0 || target >= current.length) return;
    const next = current.slice();
    [next[index], next[target]] = [next[target], next[index]];
    props.onChange(serializeAssetReferenceList(next));
  };

  const readReference = async (reference: AssetReference) => {
    const formName = props.formName?.trim();
    const entryId = props.entryId?.trim();
    if (!formName || !entryId) {
      setUnavailableIds((ids) => new Set([...ids, reference.asset_id]));
      return;
    }
    setReadingIds((ids) => new Set([...ids, reference.asset_id]));
    try {
      const blob = await assetApi.read(
        props.spaceId,
        reference.asset_id,
        formName,
        entryId,
      );
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
    } catch {
      setUnavailableIds((ids) => new Set([...ids, reference.asset_id]));
    } finally {
      setReadingIds((ids) => {
        const next = new Set(ids);
        next.delete(reference.asset_id);
        return next;
      });
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
                  !props.entryId}
              >
                {readingIds().has(reference.asset_id)
                  ? t("assetField.status.reading")
                  : t("assetField.action.download")}
              </button>
              <Show when={!props.readOnly && !props.multiple}>
                <label class="ui-button ui-button-secondary ui-button-sm">
                  {t("assetField.action.replace")}
                  <input
                    ref={replaceFileInput}
                    type="file"
                    class="ui-sr-only"
                    aria-label={t("assetField.action.replace")}
                    onChange={(event) =>
                      chooseFiles(Array.from(event.currentTarget.files ?? []))}
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
            ref={chooseFileInput}
            id={`${props.fieldId}-file`}
            type="file"
            class="ui-sr-only"
            multiple={props.multiple}
            aria-label={t("assetField.action.choose")}
            onChange={(event) =>
              chooseFiles(Array.from(event.currentTarget.files ?? []))}
          />
        </label>
        <p class="text-xs ui-muted">{t("assetField.drop")}</p>
      </Show>
    </div>
  );
}
