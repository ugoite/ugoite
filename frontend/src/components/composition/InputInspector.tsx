import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import {
  type InspectorApply,
  InspectorShell,
} from "~/components/composition/InspectorFields";
import {
  type CompositionDraft,
  setInputLabel,
  setInputVariable,
} from "~/lib/composition-draft";
import { compositionVariableDisplayName } from "~/lib/composition-variable-name";
import { t } from "~/lib/i18n";

export function InputInspector(props: {
  draft: CompositionDraft;
  inputId: string;
  apply: InspectorApply;
  actions?: JSX.Element;
}) {
  const input = () =>
    props.draft.inputs.find((item) => item.draftId === props.inputId);
  const currentVariable = () =>
    props.draft.variables.find((variable) => variable.id === input()?.variable);
  const apply = (result: ReturnType<typeof setInputLabel>) => {
    if (result.ok) props.apply(result);
  };

  return (
    <InspectorShell
      icon="canvas-input"
      title={currentVariable()
        ? compositionVariableDisplayName(
          props.draft.variables,
          currentVariable()!.id,
        )
        : t("composition.studioAddInput")}
      actions={props.actions}
    >
      <Show when={input()}>
        {(entry) => (
          <>
            <div class="ui-field">
              <label>
                <span>{t("composition.studioVariables")}</span>
                <select
                  class="ui-input"
                  value={entry().variable}
                  aria-label={t("composition.studioVariables")}
                  onChange={(event) =>
                    apply(
                      setInputVariable(
                        props.draft,
                        entry().draftId,
                        event.currentTarget.value,
                      ),
                    )}
                >
                  <For each={props.draft.variables}>
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
              </label>
            </div>
            <div class="ui-field">
              <label>
                <span>{t("composition.studioInputLabel")}</span>
                <input
                  class="ui-input"
                  value={entry().label ?? ""}
                  aria-label={t("composition.studioInputLabelName", {
                    name: currentVariable()
                      ? compositionVariableDisplayName(
                        props.draft.variables,
                        currentVariable()!.id,
                      )
                      : t("composition.studioAddInput"),
                  })}
                  onInput={(event) =>
                    apply(
                      setInputLabel(
                        props.draft,
                        entry().draftId,
                        event.currentTarget.value,
                      ),
                    )}
                />
              </label>
            </div>
          </>
        )}
      </Show>
    </InspectorShell>
  );
}
