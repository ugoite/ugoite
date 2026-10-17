import { Show } from "solid-js";
import {
  type CompositionInspectorDataJump,
  type InspectorApply,
} from "~/components/composition/InspectorFields";
import { MetricInspector } from "~/components/composition/MetricInspector";
import { InputInspector } from "~/components/composition/InputInspector";
import { TableInspector } from "~/components/composition/TableInspector";
import { TextInspector } from "~/components/composition/TextInspector";
import { IconButton } from "~/components/IconButton";
import {
  type CompositionDraft,
  type DraftDisplay,
} from "~/lib/composition-draft";
import { displayDefaultName } from "~/lib/composition-display-name";
import { compositionVariableDisplayName } from "~/lib/composition-variable-name";
import { t } from "~/lib/i18n";

export type { CompositionInspectorDataJump };

export interface CompositionInspectorProps {
  draft: CompositionDraft;
  /** Transient canvas selection; the inspector renders one block only. */
  selectedId: string | null;
  /** Single draft mutation channel shared with the canvas. */
  onDraftChange: (draft: CompositionDraft) => void;
  /** Remove the selected display or Input component. */
  onRemove: () => void;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
  onChangeTableSource: (displayId: string) => void;
}

type InspectorSelection =
  | { kind: "component"; display: DraftDisplay }
  | { kind: "input"; inputId: string };

const resolveSelection = (
  draft: CompositionDraft,
  selectedId: string | null,
): InspectorSelection | undefined => {
  if (!selectedId) return undefined;
  if (draft.inputs.some((input) => input.draftId === selectedId)) {
    return { kind: "input", inputId: selectedId };
  }
  const display = draft.displays.find((entry) => entry.draftId === selectedId);
  return display ? { kind: "component", display } : undefined;
};

/**
 * Inspector for the selected canvas block only. Metrics bind SQL source and
 * result column; tables bind source; text binds content and fixed style;
 * Inputs bind one variable. Every edit flows through draft updaters into the
 * shared debounced preview. Without a
 * resolvable selection the inspector renders nothing: no panel, no prose.
 * Every selected Component can be removed without deleting its variable.
 */
export function CompositionInspector(props: CompositionInspectorProps) {
  const selection = () => resolveSelection(props.draft, props.selectedId);

  const apply: InspectorApply = (result) => {
    if (result.ok) props.onDraftChange(result.draft);
  };

  return (
    <Show when={selection()}>
      {(entry) => (
        <Show
          when={entry().kind === "input"}
          fallback={
            <ComponentInspector
              draft={props.draft}
              display={(entry() as { display: DraftDisplay }).display}
              onDraftChange={props.onDraftChange}
              onRemove={props.onRemove}
              onDataJump={props.onDataJump}
              onChangeTableSource={props.onChangeTableSource}
            />
          }
        >
          <InputInspector
            draft={props.draft}
            inputId={(entry() as { inputId: string }).inputId}
            apply={apply}
            actions={(() => {
              const inputId = (entry() as { inputId: string }).inputId;
              const input = props.draft.inputs.find((item) =>
                item.draftId === inputId
              );
              const name = compositionVariableDisplayName(
                props.draft.variables,
                input?.variable ?? "",
              );
              const label = t("composition.studioRemoveInput", { name });
              return (
                <IconButton
                  icon="trash"
                  label={label}
                  title={label}
                  onClick={() => props.onRemove()}
                />
              );
            })()}
          />
        </Show>
      )}
    </Show>
  );
}

function ComponentInspector(props: {
  draft: CompositionDraft;
  display: DraftDisplay;
  onDraftChange: (draft: CompositionDraft) => void;
  onRemove: () => void;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
  onChangeTableSource: (displayId: string) => void;
}) {
  const display = () => props.display;
  const apply: InspectorApply = (result) => {
    if (result.ok) props.onDraftChange(result.draft);
  };
  const kind = () => display().kind;
  const removeLabel = () =>
    t("composition.studioRemoveDisplay", {
      name: displayDefaultName(
        display(),
        props.draft.sources,
        props.draft.variables,
      ),
    });
  const removeAction = () => (
    <IconButton
      icon="trash"
      label={removeLabel()}
      title={removeLabel()}
      onClick={() => props.onRemove()}
    />
  );
  return (
    <Show
      when={kind() === "text"}
      fallback={
        <Show
          when={kind() === "metric"}
          fallback={
            <TableInspector
              draft={props.draft}
              display={display()}
              actions={removeAction()}
              onDataJump={props.onDataJump}
              onChangeSource={props.onChangeTableSource}
            />
          }
        >
          <MetricInspector
            draft={props.draft}
            display={display()}
            apply={apply}
            actions={removeAction()}
            onDataJump={props.onDataJump}
          />
        </Show>
      }
    >
      <TextInspector
        draft={props.draft}
        display={display()}
        apply={apply}
        actions={removeAction()}
      />
    </Show>
  );
}
