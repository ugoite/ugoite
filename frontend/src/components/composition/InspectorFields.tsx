import { For } from "solid-js";
import type { JSX } from "solid-js";
import { UiIcon, type UiIconName } from "~/components/UiIcon";
import {
  type CompositionDraft,
  type DraftDisplay,
  type DraftResult,
  setDisplayLabel,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

/**
 * Minimal typed handoff for the RA6 Data workspace and RA7 split sync: the
 * inspector names the block's source, and the Studio owns expanding,
 * scrolling to, and focusing the existing source row.
 */
export interface CompositionInspectorDataJump {
  readonly sourceDraftId: string;
}

export type InspectorApply = (result: DraftResult) => void;

export function InspectorShell(props: {
  icon: Extract<
    UiIconName,
    "canvas-text" | "canvas-metric" | "canvas-table" | "canvas-input"
  >;
  title: string;
  children: JSX.Element;
}) {
  return (
    <aside
      class="studioInspector"
      aria-labelledby="studio-inspector-heading"
    >
      <h3 id="studio-inspector-heading" class="studioInspectorTitle">
        <UiIcon name={props.icon} />
        <span>{props.title}</span>
      </h3>
      {props.children}
    </aside>
  );
}

export function InspectorLabelField(props: {
  display: DraftDisplay;
  draft: CompositionDraft;
  apply: InspectorApply;
}) {
  const fieldId = () => `studio-inspector-label-${props.display.draftId}`;
  return (
    <div class="ui-field">
      <label class="ui-label" for={fieldId()}>
        {t("composition.studioLabel")}
      </label>
      <input
        id={fieldId()}
        class="ui-input"
        value={props.display.label ?? ""}
        onInput={(event) =>
          props.apply(
            setDisplayLabel(
              props.draft,
              props.display.draftId,
              event.currentTarget.value,
            ),
          )}
      />
    </div>
  );
}

export function InspectorSourceField(props: {
  draft: CompositionDraft;
  sourceDraftId: string;
  fieldId: string;
  onPick: (sourceDraftId: string) => void;
}) {
  return (
    <div class="ui-field">
      <label class="ui-label" for={props.fieldId}>
        {t("composition.studioSource")}
      </label>
      <select
        id={props.fieldId}
        class="ui-input"
        value={props.sourceDraftId}
        onChange={(event) => props.onPick(event.currentTarget.value)}
      >
        <For each={props.draft.sources}>
          {(source) => <option value={source.draftId}>{source.name}</option>}
        </For>
      </select>
    </div>
  );
}

export function InspectorDataJumpButton(props: {
  sourceDraftId: string;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
}) {
  return (
    <div class="studioInspectorActions">
      <button
        type="button"
        class="pill"
        onClick={() => props.onDataJump({ sourceDraftId: props.sourceDraftId })}
      >
        <span aria-hidden="true">↗</span>
        <span>{t("composition.studioData")}</span>
      </button>
    </div>
  );
}
