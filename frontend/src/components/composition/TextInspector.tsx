import { createEffect, createSignal, For, Show } from "solid-js";
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
 * Text inspector: edit the literal body directly, or explicitly bind it to a
 * string variable. Text edits never touch sources and never refetch.
 */
export function TextInspector(props: {
  draft: CompositionDraft;
  display: DraftDisplay;
  apply: InspectorApply;
  textLiteralCache: Map<string, string>;
  actions?: JSX.Element;
}) {
  const text = () => props.display.kind === "text" ? props.display : undefined;
  const stringVariables = () =>
    props.draft.variables.filter((item) => item.type === "string");
  const [choosingVariable, setChoosingVariable] = createSignal(false);
  const [focusTarget, setFocusTarget] = createSignal<
    "variable" | "action" | "body" | undefined
  >();
  let textarea: HTMLTextAreaElement | undefined;
  let variableSelect: HTMLSelectElement | undefined;
  let variableAction: HTMLButtonElement | undefined;

  createEffect(() => {
    const target = focusTarget();
    if (target === "variable") variableSelect?.focus();
    if (target === "action") variableAction?.focus();
    if (target === "body") textarea?.focus();
    if (target) setFocusTarget(undefined);
  });

  const resizeBody = () => {
    if (!textarea) return;
    textarea.style.height = "auto";
    textarea.style.height = `${textarea.scrollHeight}px`;
  };

  return (
    <InspectorShell
      icon="canvas-text"
      title={t("composition.studioText")}
      actions={props.actions}
    >
      <Show when={text()}>
        {(entry) => {
          const bodyId = `studio-inspector-text-body-${entry().draftId}`;
          const variableId =
            `studio-inspector-text-variable-${entry().draftId}`;
          const styleId = `studio-inspector-style-${entry().draftId}`;
          const bodyText = () =>
            entry().content.kind === "literal" ? entry().content.text : "";

          createEffect(() => {
            const current = entry();
            if (current.content.kind === "literal") {
              props.textLiteralCache.set(current.draftId, current.content.text);
            }
            bodyText();
            queueMicrotask(resizeBody);
          });

          return (
            <>
              <Show
                when={entry().content.kind === "literal"}
                fallback={
                  <div class="ui-field">
                    <div class="ui-field-value-text">
                      {entry().content.kind === "variable"
                        ? compositionVariableDisplayName(
                          props.draft.variables,
                          entry().content.variable,
                        )
                        : ""}
                    </div>
                  </div>
                }
              >
                <div class="ui-field">
                  <label class="ui-sr-only" for={bodyId}>
                    {t("composition.studioTextBody")}
                  </label>
                  <textarea
                    ref={textarea}
                    id={bodyId}
                    class="ui-input ui-textarea-auto"
                    rows="1"
                    value={bodyText()}
                    onInput={(event) => {
                      props.textLiteralCache.set(
                        entry().draftId,
                        event.currentTarget.value,
                      );
                      resizeBody();
                      props.apply(
                        setTextContent(
                          props.draft,
                          entry().draftId,
                          event.currentTarget.value,
                        ),
                      );
                    }}
                  />
                </div>
              </Show>

              <Show when={stringVariables().length > 0}>
                <div class="ui-field">
                  <Show
                    when={choosingVariable() &&
                      entry().content.kind === "literal"}
                    fallback={null}
                  >
                    <label class="ui-sr-only" for={variableId}>
                      {t("composition.studioTextVariable")}
                    </label>
                    <select
                      ref={variableSelect}
                      id={variableId}
                      class="ui-input"
                      value=""
                      onChange={(event) => {
                        const selected = event.currentTarget.value;
                        if (!selected) return;
                        props.apply(
                          setTextVariable(
                            props.draft,
                            entry().draftId,
                            selected,
                          ),
                        );
                        setChoosingVariable(false);
                        setFocusTarget("action");
                      }}
                      onKeyDown={(event) => {
                        if (event.key === "Escape") {
                          event.preventDefault();
                          event.stopPropagation();
                          setChoosingVariable(false);
                          setFocusTarget("action");
                        }
                      }}
                    >
                      <option value="" disabled>
                        {t("composition.studioChooseVariable")}
                      </option>
                      <For each={stringVariables()}>
                        {(variable) => (
                          <option value={variable.id}>
                            {compositionVariableDisplayName(
                              props.draft.variables,
                              variable.id,
                            )}
                          </option>
                        )}
                      </For>
                    </select>
                  </Show>
                  <Show when={!choosingVariable() ||
                    entry().content.kind === "variable"}>
                    <button
                      ref={variableAction}
                      type="button"
                      class="pill"
                      onClick={() => {
                        if (entry().content.kind === "variable") {
                          props.apply(
                            setTextContent(
                              props.draft,
                              entry().draftId,
                              props.textLiteralCache.get(entry().draftId) ?? "",
                            ),
                          );
                          setChoosingVariable(false);
                          setFocusTarget("body");
                        } else {
                          setChoosingVariable(true);
                          setFocusTarget("variable");
                        }
                      }}
                    >
                      {entry().content.kind === "variable"
                        ? t("composition.studioTextUseLiteral")
                        : t("composition.studioTextUseVariable")}
                    </button>
                  </Show>
                </div>
              </Show>

              <div class="ui-field">
                <label class="ui-label" for={styleId}>
                  {t("composition.studioStyle")}
                </label>
                <select
                  id={styleId}
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
          );
        }}
      </Show>
    </InspectorShell>
  );
}
