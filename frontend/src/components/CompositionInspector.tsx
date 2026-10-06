import { Show } from "solid-js";
import {
  type CompositionInspectorDataJump,
  type InspectorApply,
} from "~/components/composition/InspectorFields";
import { MetricInspector } from "~/components/composition/MetricInspector";
import { ParameterPlacementInspector } from "~/components/composition/ParameterPlacementInspector";
import { TableInspector } from "~/components/composition/TableInspector";
import { TextInspector } from "~/components/composition/TextInspector";
import {
  type CompositionDraft,
  type DraftDisplay,
} from "~/lib/composition-draft";

const PARAMETER_BLOCK_PREFIX = "param:";

export type { CompositionInspectorDataJump };

export interface CompositionInspectorProps {
  draft: CompositionDraft;
  /** Transient canvas selection; the inspector renders one block only. */
  selectedId: string | null;
  /** Single draft mutation channel shared with the canvas. */
  onDraftChange: (draft: CompositionDraft) => void;
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
              onDraftChange={props.onDraftChange}
              onDataJump={props.onDataJump}
            />
          }
        >
          <ParameterPlacementInspector
            draft={props.draft}
            parameterId={(entry() as { parameterId: string }).parameterId}
            apply={apply}
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
  onDataJump: (jump: CompositionInspectorDataJump) => void;
}) {
  const display = () => props.display;
  const apply: InspectorApply = (result) => {
    if (result.ok) props.onDraftChange(result.draft);
  };
  const kind = () => display().kind;
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
              onDataJump={props.onDataJump}
            />
          }
        >
          <MetricInspector
            draft={props.draft}
            display={display()}
            apply={apply}
            onDataJump={props.onDataJump}
          />
        </Show>
      }
    >
      <TextInspector draft={props.draft} display={display()} apply={apply} />
    </Show>
  );
}
