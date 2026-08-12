import {
  createMemo,
  createResource,
  createSignal,
  For,
  Match,
  Show,
  Switch,
} from "solid-js";
import type { JSX } from "solid-js";

import { renderMarkdownPreview } from "~/lib/markdown";
import {
  formatJsonPreview,
  MAX_PREVIEW_BYTES,
  parseDelimitedPreview,
  readPreviewText,
  resolvePreviewKind,
} from "~/lib/asset-preview";
import { t } from "~/lib/i18n";
import type { AssetReference } from "~/lib/types";

export interface AssetPreviewProps {
  reference: AssetReference;
  blob: Blob;
  url: string;
}

function TextResourcePreview(props: {
  blob: Blob;
  render: (text: string) => JSX.Element;
}) {
  const [text] = createResource(() => props.blob, readPreviewText);

  return (
    <Show
      when={!text.loading}
      fallback={
        <p class="text-sm ui-muted">{t("assetField.preview.loading")}</p>
      }
    >
      <Show
        when={!text.error}
        fallback={
          <p class="ui-alert ui-alert-error text-sm" role="alert">
            {t("assetField.preview.failed")}
          </p>
        }
      >
        <div>
          {props.render(text() ?? "")}
          <Show when={props.blob.size > MAX_PREVIEW_BYTES}>
            <p class="mt-2 text-xs ui-muted">
              {t("assetField.preview.truncated")}
            </p>
          </Show>
        </div>
      </Show>
    </Show>
  );
}

function DelimitedPreview(props: { blob: Blob; delimiter: "," | "\t" }) {
  return (
    <TextResourcePreview
      blob={props.blob}
      render={(text) => {
        const table = parseDelimitedPreview(text, props.delimiter);
        const [header = [], ...body] = table.rows;
        return (
          <Show
            when={header.length > 0}
            fallback={
              <p class="text-sm ui-muted">{t("assetField.preview.empty")}</p>
            }
          >
            <div class="ui-asset-table-wrap">
              <table class="ui-asset-preview-table">
                <thead>
                  <tr>
                    <For each={header}>
                      {(cell) => <th scope="col">{cell}</th>}
                    </For>
                  </tr>
                </thead>
                <tbody>
                  <For each={body}>
                    {(row) => (
                      <tr>
                        <For each={row}>
                          {(cell) => <td>{cell}</td>}
                        </For>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
            <Show when={table.truncatedRows || table.truncatedColumns}>
              <p class="mt-2 text-xs ui-muted">
                {t("assetField.preview.tableLimited")}
              </p>
            </Show>
          </Show>
        );
      }}
    />
  );
}

function NativeMediaPreview(props: {
  kind: "audio" | "video";
  url: string;
}) {
  const [failed, setFailed] = createSignal(false);
  const unsupportedKey = () =>
    props.kind === "audio"
      ? "assetField.preview.audioUnsupported"
      : "assetField.preview.videoUnsupported";

  return (
    <Show
      when={!failed()}
      fallback={<p class="text-sm ui-muted">{t(unsupportedKey())}</p>}
    >
      <Show
        when={props.kind === "audio"}
        fallback={
          <video
            class="ui-asset-media-preview"
            controls
            preload="metadata"
            src={props.url}
            onError={() => setFailed(true)}
          />
        }
      >
        <audio
          class="ui-asset-media-preview"
          controls
          preload="metadata"
          src={props.url}
          onError={() => setFailed(true)}
        />
      </Show>
    </Show>
  );
}

/** Render one already-authorized asset with browser-native preview primitives. */
export function AssetPreview(props: AssetPreviewProps) {
  const kind = createMemo(() => resolvePreviewKind(props.reference));

  return (
    <div class="ui-asset-preview-panel">
      <Switch>
        <Match when={kind() === "image"}>
          <img
            class="ui-asset-image-preview"
            src={props.url}
            alt={props.reference.name}
          />
        </Match>
        <Match when={kind() === "pdf"}>
          <iframe
            class="ui-asset-document-preview"
            src={props.url}
            title={props.reference.name}
          />
        </Match>
        <Match when={kind() === "text"}>
          <TextResourcePreview
            blob={props.blob}
            render={(text) => <pre class="ui-asset-text-preview">{text}</pre>}
          />
        </Match>
        <Match when={kind() === "json"}>
          <TextResourcePreview
            blob={props.blob}
            render={(text) => (
              <pre class="ui-asset-text-preview">{formatJsonPreview(text)}</pre>
            )}
          />
        </Match>
        <Match when={kind() === "markdown"}>
          <TextResourcePreview
            blob={props.blob}
            render={(text) => (
              <div
                class="ui-preview ui-asset-markdown-preview"
                innerHTML={renderMarkdownPreview(text)}
              />
            )}
          />
        </Match>
        <Match when={kind() === "csv"}>
          <DelimitedPreview
            blob={props.blob}
            delimiter={props.reference.name.toLowerCase().endsWith(".tsv")
              ? "\t"
              : ","}
          />
        </Match>
        <Match when={kind() === "audio"}>
          <NativeMediaPreview kind="audio" url={props.url} />
        </Match>
        <Match when={kind() === "video"}>
          <NativeMediaPreview kind="video" url={props.url} />
        </Match>
        <Match when={true}>
          <p class="text-sm ui-muted">{t("assetField.preview.unsupported")}</p>
        </Match>
      </Switch>
    </div>
  );
}
