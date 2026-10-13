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
  setTextVariable,
} from "~/lib/composition-draft";
import type { CompositionTextStyle } from "~/lib/composition-api";
import { compositionVariableDisplayName } from "~/lib/composition-variable-name";
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
                for={`studio-inspector-text-content-${entry().draftId}`}
              >
                {t("composition.studioTextContent")}
              </label>
              <select
                id={`studio-inspector-text-content-${entry().draftId}`}
                class="ui-input"
                value={entry().content.kind === "literal"
                  ? "literal"
                  : `variable:${entry().content.variable}`}
                onChange={(event) => {
                  const selected = event.currentTarget.value;
                  if (selected === "literal") {
                    props.apply(
                      setTextContent(props.draft, entry().draftId, ""),
                    );
                    return;
                  }
                  if (selected.startsWith("variable:")) {
                    props.apply(
                      setTextVariable(
                        props.draft,
                        entry().draftId,
                        selected.slice("variable:".length),
                      ),
                    );
                  }
                }}
              >
                <option value="literal">
                  {t("composition.studioTextLiteral")}
                </option>
                <For
                  each={props.draft.variables.filter((item) =>
                    item.type === "string"
                  )}
                >
                  {(variable) => (
                    <option value={`variable:${variable.id}`}>
                      {compositionVariableDisplayName(
                        props.draft.variables,
                        variable.id,
                      )}
                    </option>
                  )}
                </For>
              </select>
            </div>
            <div class="ui-field">
              <label
                class="ui-label"
                for={`studio-inspector-text-${entry().draftId}`}
              >
                {t("composition.studioText")}
              </label>
              <Show when={entry().content.kind === "literal"}>
                <input
                  id={`studio-inspector-text-${entry().draftId}`}
                  class="ui-input"
                  value={entry().content.kind === "literal"
                    ? entry().content.text
                    : ""}
                  onInput={(event) =>
                    props.apply(
                      setTextContent(
                        props.draft,
                        entry().draftId,
                        event.currentTarget.value,
                      ),
                    )}
                />
              </Show>
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
