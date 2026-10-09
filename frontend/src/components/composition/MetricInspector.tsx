import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import {
  type CompositionInspectorDataJump,
  type InspectorApply,
  InspectorDataJumpButton,
  InspectorLabelField,
  InspectorShell,
  InspectorSourceField,
} from "~/components/composition/InspectorFields";
import { displayValueName } from "~/lib/composition-display-name";
import {
  type DisplayScalarCandidate,
  displayScalarCandidates,
} from "~/components/CompositionDisplayPicker";
import {
  type CompositionDraft,
  type DraftDisplay,
  type DraftMetricValueField,
  setMetricSource,
  setMetricValueField,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

const sameValueField = (
  left: DraftMetricValueField,
  right: DraftMetricValueField,
): boolean =>
  "fieldId" in left
    ? "fieldId" in right && left.fieldId === right.fieldId
    : "column" in right && left.column === right.column;

const candidateIdForValueField = (
  valueField: DraftMetricValueField,
): string =>
  "fieldId" in valueField ? String(valueField.fieldId) : valueField.column;

/**
 * Metric inspector: label input, data source picker over the draft's
 * existing sources, and a value selector over entry fields (form schema) or
 * SQL columns (expected_result) with no type inference. Changing the source
 * keeps the value field when it stays valid and otherwise takes the new
 * source's first scalar candidate; a candidate-less source keeps the stale
 * value so resolve diagnostics stay visible instead of guessing.
 */
export function MetricInspector(props: {
  draft: CompositionDraft;
  display: DraftDisplay;
  fieldNames?: (formId: string, fieldId: number) => string | undefined;
  fieldProjectable?: (formId: string, fieldId: number) => boolean | undefined;
  apply: InspectorApply;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
  actions?: JSX.Element;
}) {
  const metric = () =>
    props.display.kind === "metric" ? props.display : undefined;
  const sourceId = () => (metric() as { sourceDraftId: string }).sourceDraftId;
  const candidates = (): DisplayScalarCandidate[] => {
    const source = props.draft.sources.find((entry) =>
      entry.draftId === sourceId()
    );
    return source
      ? displayScalarCandidates(
        source,
        props.fieldNames,
        props.fieldProjectable,
      )
      : [];
  };
  const currentValueId = () =>
    candidateIdForValueField(
      (metric() as { valueField: DraftMetricValueField }).valueField,
    );
  const matchedValueId = () =>
    candidates().some((candidate) => candidate.id === currentValueId())
      ? currentValueId()
      : "";

  const pickSource = (nextSourceId: string) => {
    const current = metric();
    if (!current || nextSourceId === current.sourceDraftId) return;
    const next = props.draft.sources.find((entry) =>
      entry.draftId === nextSourceId
    );
    if (!next) return;
    const nextCandidates = displayScalarCandidates(
      next,
      props.fieldNames,
      props.fieldProjectable,
    );
    const kept =
      nextCandidates.some((candidate) =>
          sameValueField(candidate.valueField, current.valueField)
        )
        ? current.valueField
        : nextCandidates[0]?.valueField ?? current.valueField;
    props.apply(
      setMetricSource(props.draft, current.draftId, nextSourceId, kept),
    );
  };

  const pickValue = (valueId: string) => {
    const current = metric();
    if (!current) return;
    const candidate = candidates().find((entry) => entry.id === valueId);
    if (!candidate || candidate.id === currentValueId()) return;
    props.apply(
      setMetricValueField(props.draft, current.draftId, candidate.valueField),
    );
  };

  return (
    <InspectorShell
      icon="canvas-metric"
      title={t("composition.studioMetric")}
      actions={props.actions}
    >
      <Show when={metric()}>
        {(entry) => (
          <>
            <InspectorLabelField
              display={entry()}
              draft={props.draft}
              apply={props.apply}
            />
            <InspectorSourceField
              draft={props.draft}
              sourceDraftId={sourceId()}
              fieldId={`studio-inspector-source-${entry().draftId}`}
              onPick={pickSource}
            />
            <div class="ui-field">
              <label
                class="ui-label"
                for={`studio-inspector-value-${entry().draftId}`}
              >
                {t("composition.studioValue")}
              </label>
              <select
                id={`studio-inspector-value-${entry().draftId}`}
                class="ui-input"
                value={matchedValueId()}
                onChange={(event) => pickValue(event.currentTarget.value)}
              >
                <Show when={!matchedValueId()}>
                  <option value="" disabled>
                    {displayValueName(
                      entry(),
                      props.draft.sources,
                      props.fieldNames,
                    )}
                  </option>
                </Show>
                <For each={candidates()}>
                  {(candidate) => (
                    <option value={candidate.id}>{candidate.name}</option>
                  )}
                </For>
              </select>
            </div>
            <InspectorDataJumpButton
              sourceDraftId={sourceId()}
              onDataJump={props.onDataJump}
            />
          </>
        )}
      </Show>
    </InspectorShell>
  );
}
