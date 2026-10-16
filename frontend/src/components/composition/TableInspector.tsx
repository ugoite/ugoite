import { Show } from "solid-js";
import type { JSX } from "solid-js";
import {
  type CompositionInspectorDataJump,
  type InspectorApply,
  InspectorDataJumpButton,
  InspectorShell,
  InspectorSourceField,
} from "~/components/composition/InspectorFields";
import {
  type CompositionDraft,
  type DraftDisplay,
  setTableSource,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

/**
 * Table inspector: data source picker over the draft's existing sources.
 * Tables render whole source-native
 * result pages, and no source is created here.
 */
export function TableInspector(props: {
  draft: CompositionDraft;
  display: DraftDisplay;
  apply: InspectorApply;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
  actions?: JSX.Element;
}) {
  const table = () =>
    props.display.kind === "table" ? props.display : undefined;
  return (
    <InspectorShell
      icon="canvas-table"
      title={t("composition.studioTable")}
      actions={props.actions}
    >
      <Show when={table()}>
        {(entry) => (
          <>
            <InspectorSourceField
              draft={props.draft}
              sourceDraftId={entry().sourceDraftId}
              fieldId={`studio-inspector-source-${entry().draftId}`}
              onPick={(nextSourceId) => {
                if (nextSourceId === entry().sourceDraftId) return;
                props.apply(
                  setTableSource(props.draft, entry().draftId, nextSourceId),
                );
              }}
            />
            <InspectorDataJumpButton
              sourceDraftId={entry().sourceDraftId}
              onDataJump={props.onDataJump}
            />
          </>
        )}
      </Show>
    </InspectorShell>
  );
}
