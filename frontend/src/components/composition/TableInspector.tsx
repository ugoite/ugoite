import { Show } from "solid-js";
import type { JSX } from "solid-js";
import {
  type CompositionInspectorDataJump,
  InspectorDataJumpButton,
  InspectorShell,
} from "~/components/composition/InspectorFields";
import { UiIcon } from "~/components/UiIcon";
import {
  type CompositionDraft,
  type DraftDisplay,
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
  onDataJump: (jump: CompositionInspectorDataJump) => void;
  onChangeSource: (displayId: string) => void;
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
            <button
              class="pill"
              type="button"
              onClick={() => props.onChangeSource(entry().draftId)}
            >
              <UiIcon name="sql" />
              <span>{t("composition.studioChangeTableSource")}</span>
            </button>
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
