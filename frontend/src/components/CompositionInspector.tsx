import { Show } from "solid-js";
import {
  type CompositionInspectorDataJump,
  type InspectorApply,
} from "~/components/composition/InspectorFields";
import { MetricInspector } from "~/components/composition/MetricInspector";
import { ParameterPlacementInspector } from "~/components/composition/ParameterPlacementInspector";
import { TableInspector } from "~/components/composition/TableInspector";
import { TextInspector } from "~/components/composition/TextInspector";
import { IconButton } from "~/components/IconButton";
import {
  type CompositionDraft,
  type DraftDisplay,
} from "~/lib/composition-draft";
import type { CompositionFieldNames } from "~/components/CompositionRenderer";
import { displayDefaultName } from "~/lib/composition-display-name";
import { t } from "~/lib/i18n";

const PARAMETER_BLOCK_PREFIX = "param:";

export type { CompositionInspectorDataJump };

export interface CompositionInspectorProps {
  draft: CompositionDraft;
  /** Transient canvas selection; the inspector renders one block only. */
  selectedId: string | null;
  fieldNames?: CompositionFieldNames;
  fieldProjectable?: (formId: string, fieldId: number) => boolean | undefined;
  /** Single draft mutation channel shared with the canvas. */
  onDraftChange: (draft: CompositionDraft) => void;
  /** Remove the selected display or unplace the selected parameter control. */
  onRemove: () => void;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
}

type InspectorSelection =
  | { kind: "component"; display: DraftDisplay }
  | { kind: "parameter"; parameterId: string };

const resolveSelection = (
  draft: CompositionDraft,
  selectedId: string | null,
): InspectorSelection | undefined => {
  if (!selectedId) return undefined;
  if (selectedId.startsWith(PARAMETER_BLOCK_PREFIX)) {
    const parameterId = selectedId.slice(PARAMETER_BLOCK_PREFIX.length);
    if (!draft.parameters.some((parameter) => parameter.id === parameterId)) {
      return undefined;
    }
    const placed = draft.layoutRows.some((row) =>
      row.items.some((item) =>
        item.kind === "parameter" && item.parameterId === parameterId
      )
    );
    return placed ? { kind: "parameter", parameterId } : undefined;
  }
  const display = draft.displays.find((entry) => entry.draftId === selectedId);
  return display ? { kind: "component", display } : undefined;
};

/**
 * Inspector for the selected canvas block only. Metric edits bind label,
 * source, and value field; table edits bind label and source; text edits
 * bind content and fixed style; parameter controls show the owned parameter
 * ref and retarget placement without any label override. Every edit flows
 * through draft updaters into the shared debounced preview; text, label,
 * and style edits never reshape sources, so they never refetch. Without a
 * resolvable selection the inspector renders nothing: no panel, no prose.
 * Selected displays are removed here; selected parameter controls are only
 * unplaced, leaving their declarations in Parameters.
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
          when={entry().kind === "parameter"}
          fallback={
            <ComponentInspector
              draft={props.draft}
              display={(entry() as { display: DraftDisplay }).display}
              fieldNames={props.fieldNames}
              fieldProjectable={props.fieldProjectable}
              onDraftChange={props.onDraftChange}
              onRemove={props.onRemove}
              onDataJump={props.onDataJump}
            />
          }
        >
          <ParameterPlacementInspector
            draft={props.draft}
            parameterId={(entry() as { parameterId: string }).parameterId}
            apply={apply}
            actions={(() => {
              const parameterId = (entry() as {
                parameterId: string;
              }).parameterId;
              const name =
                props.draft.parameters.find((parameter) =>
                  parameter.id === parameterId
                )?.label ?? t("composition.studioParameters");
              const label = t("composition.studioUnplaceParameter", { name });
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
  fieldNames?: CompositionFieldNames;
  fieldProjectable?: (formId: string, fieldId: number) => boolean | undefined;
  onDraftChange: (draft: CompositionDraft) => void;
  onRemove: () => void;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
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
        props.fieldNames,
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
              apply={apply}
              actions={removeAction()}
              onDataJump={props.onDataJump}
            />
          }
        >
          <MetricInspector
            draft={props.draft}
            display={display()}
            fieldNames={props.fieldNames}
            fieldProjectable={props.fieldProjectable}
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
