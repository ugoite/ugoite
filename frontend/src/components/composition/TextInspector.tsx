import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import {
  type InspectorApply,
  InspectorShell,
} from "~/components/composition/InspectorFields";
import {
  type CompositionDraft,
  type DraftDisplay,
  setTextContent,
  setTextStyle,
} from "~/lib/composition-draft";
import type { CompositionTextStyle } from "~/lib/composition-api";
import { t } from "~/lib/i18n";

const textStyles: readonly CompositionTextStyle[] = [
  "title",
  "heading",
  "body",
  "caption",
];

const textStyleLabel = (style: CompositionTextStyle): string => {
  switch (style) {
    case "title":
      return t("composition.textStyle.title");
    case "heading":
      return t("composition.textStyle.heading");
    case "body":
      return t("composition.textStyle.body");
    case "caption":
      return t("composition.textStyle.caption");
  }
};

/**
 * Text inspector: content input and a fixed style selector. Text carries no
 * source binding, so edits never touch sources and never refetch.
 */
export function TextInspector(props: {
  draft: CompositionDraft;
  display: DraftDisplay;
  apply: InspectorApply;
  actions?: JSX.Element;
}) {
  const text = () => props.display.kind === "text" ? props.display : undefined;
  return (
    <InspectorShell
      icon="canvas-text"
      title={t("composition.studioText")}
      actions={props.actions}
    >
      <Show when={text()}>
        {(entry) => (
          <>
            <div class="ui-field">
              <label
                class="ui-label"
                for={`studio-inspector-text-${entry().draftId}`}
              >
                {t("composition.studioText")}
              </label>
              <input
                id={`studio-inspector-text-${entry().draftId}`}
                class="ui-input"
                value={entry().text}
                onInput={(event) =>
                  props.apply(
                    setTextContent(
                      props.draft,
                      entry().draftId,
                      event.currentTarget.value,
                    ),
                  )}
              />
            </div>
            <div class="ui-field">
              <label
                class="ui-label"
                for={`studio-inspector-style-${entry().draftId}`}
              >
                {t("composition.studioStyle")}
              </label>
              <select
                id={`studio-inspector-style-${entry().draftId}`}
                class="ui-input"
                value={entry().style}
                onChange={(event) =>
                  props.apply(
                    setTextStyle(
                      props.draft,
                      entry().draftId,
                      event.currentTarget.value as CompositionTextStyle,
                    ),
                  )}
              >
                <For each={textStyles}>
                  {(style) => (
                    <option value={style}>{textStyleLabel(style)}</option>
                  )}
                </For>
              </select>
            </div>
          </>
        )}
      </Show>
    </InspectorShell>
  );
}
