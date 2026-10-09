import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import {
  type InspectorApply,
  InspectorShell,
} from "~/components/composition/InspectorFields";
import {
  type CompositionDraft,
  retargetParameterControl,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

/**
 * Parameter control inspector: the semantic parameter ref renders as the
 * owned label with no override input, and placement retargets to another
 * declared parameter in place. Declarations stay owned by the Parameters
 * section; the inspector never creates parameters or edits their labels.
 */
export function ParameterPlacementInspector(props: {
  draft: CompositionDraft;
  parameterId: string;
  apply: InspectorApply;
  actions?: JSX.Element;
}) {
  const definition = () =>
    props.draft.parameters.find((parameter) =>
      parameter.id === props.parameterId
    );
  return (
    <InspectorShell
      icon="canvas-input"
      title={definition()?.label ?? props.parameterId}
      actions={props.actions}
    >
      <Show when={definition()}>
        {(entry) => (
          <div class="ui-field">
            <label
              class="ui-label"
              for={`studio-inspector-parameter-${entry().id}`}
            >
              {t("composition.studioParameters")}
            </label>
            <select
              id={`studio-inspector-parameter-${entry().id}`}
              class="ui-input"
              value={entry().id}
              onChange={(event) => {
                if (event.currentTarget.value === entry().id) return;
                props.apply(
                  retargetParameterControl(
                    props.draft,
                    entry().id,
                    event.currentTarget.value,
                  ),
                );
              }}
            >
              <For each={props.draft.parameters}>
                {(parameter) => (
                  <option value={parameter.id}>
                    {parameter.label ?? parameter.id}
                  </option>
                )}
              </For>
            </select>
          </div>
        )}
      </Show>
    </InspectorShell>
  );
}
