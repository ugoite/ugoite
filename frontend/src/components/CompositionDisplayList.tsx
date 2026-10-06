import { createSignal, For, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { t } from "~/lib/i18n";
import type { DraftDisplay, DraftSource } from "~/lib/composition-draft";

interface CompositionDisplayListProps {
  sources: readonly DraftSource[];
  displays: readonly DraftDisplay[];
  headingId: string;
  onRemove: (displayDraftId: string) => void;
  onMove: (displayDraftId: string, direction: "up" | "down") => void;
  onChangeLabel: (displayDraftId: string, label: string) => void;
}

export const displayValueName = (display: DraftDisplay): string => {
  if (display.kind === "text") return "";
  return display.kind === "table"
    ? ""
    : "column" in display.valueField
    ? display.valueField.column
    : `#${display.valueField.fieldId}`;
};

const sourceName = (
  sources: readonly DraftSource[],
  sourceDraftId: string,
): string =>
  sources.find((source) => source.draftId === sourceDraftId)?.name ?? "";

/** Default row name: the label, else the source name (plus value for metrics). */
export const displayDefaultName = (
  display: DraftDisplay,
  sources: readonly DraftSource[],
): string => {
  if (display.label) return display.label;
  if (display.kind === "text") return display.text || display.draftId;
  const name = sourceName(sources, display.sourceDraftId);
  if (display.kind === "table") return name;
  const value = displayValueName(display);
  return value ? `${name} / ${value}` : name;
};

const displayKindLabel = (display: DraftDisplay): string =>
  display.kind === "table"
    ? t("composition.studioTable")
    : display.kind === "metric"
    ? t("composition.studioMetric")
    : t("composition.studioText");

/**
 * Display section content for the Composition Studio. Rows carry the single
 * display name owner with full-row activation toggling the detail, native
 * up/down buttons for keyboard reorder, and an inline label input.
 */
export function CompositionDisplayList(props: CompositionDisplayListProps) {
  const [expandedId, setExpandedId] = createSignal<string | null>(null);

  const toggleExpanded = (displayDraftId: string) => {
    setExpandedId((current) =>
      current === displayDraftId ? null : displayDraftId
    );
  };

  const expandedDisplay = (): DraftDisplay | undefined =>
    props.displays.find((display) => display.draftId === expandedId());

  return (
    <Show
      when={props.displays.length > 0}
      fallback={<p class="ui-muted">{t("composition.studioEmptyDisplay")}</p>}
    >
      <RowList
        label={t("composition.studioDisplay")}
        labelledBy={props.headingId}
      >
        <For each={props.displays}>
          {(display, index) => (
            <RowListItem
              main={
                <RowListButton
                  ariaLabel={displayDefaultName(display, props.sources)}
                  primary={
                    <span class="rowListName">
                      <span>
                        {displayDefaultName(display, props.sources)}
                      </span>
                    </span>
                  }
                  secondary={displayKindLabel(display)}
                  onActivate={() => toggleExpanded(display.draftId)}
                />
              }
              actions={
                <>
                  <button
                    type="button"
                    class="pill iconpill icononly"
                    disabled={index() === 0}
                    aria-label={t("composition.studioMoveUp", {
                      name: displayDefaultName(display, props.sources),
                    })}
                    onClick={() => props.onMove(display.draftId, "up")}
                  >
                    <span aria-hidden="true">↑</span>
                  </button>
                  <button
                    type="button"
                    class="pill iconpill icononly"
                    disabled={index() === props.displays.length - 1}
                    aria-label={t("composition.studioMoveDown", {
                      name: displayDefaultName(display, props.sources),
                    })}
                    onClick={() => props.onMove(display.draftId, "down")}
                  >
                    <span aria-hidden="true">↓</span>
                  </button>
                  <IconButton
                    icon="trash"
                    label={t("composition.studioRemoveDisplay", {
                      name: displayDefaultName(display, props.sources),
                    })}
                    onClick={() => props.onRemove(display.draftId)}
                  />
                </>
              }
            />
          )}
        </For>
      </RowList>
      <Show when={expandedDisplay()}>
        {(display) => (
          <details class="ui-stack-sm" open>
            <summary>
              {displayDefaultName(display(), props.sources)}
            </summary>
            <label
              class="ui-label"
              for={`studio-display-label-${display().draftId}`}
            >
              {t("composition.studioLabel")}
            </label>
            <input
              id={`studio-display-label-${display().draftId}`}
              class="ui-input"
              value={display().label ?? ""}
              onInput={(event) =>
                props.onChangeLabel(
                  display().draftId,
                  event.currentTarget.value,
                )}
            />
            <Show when={display().kind === "metric"}>
              <div class="ui-muted">
                {displayValueName(display())}
              </div>
            </Show>
          </details>
        )}
      </Show>
    </Show>
  );
}
