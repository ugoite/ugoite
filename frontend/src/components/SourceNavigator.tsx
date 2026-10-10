import { For, Show } from "solid-js";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import type { DraftSource } from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

export const sourceKindIcon = (source: DraftSource): "sql" | "forms" =>
  source.kind === "saved_sql" ? "sql" : "forms";

export const sourceKindLabel = (source: DraftSource): string =>
  source.kind === "saved_sql"
    ? t("spaceShell.title.savedSql")
    : t("common.form");

interface SourceNavigatorProps {
  sources: readonly DraftSource[];
  headingId: string;
  /** Currently selected source; the editor area renders its detail. */
  selectedId: string | null;
  onSelect: (sourceDraftId: string) => void;
  /** Registers each row element so the inspector data jump can focus it. */
  registerRow: (sourceDraftId: string, el: HTMLDivElement | null) => void;
}

/**
 * Data source navigator for the Composition Studio Data workspace.
 * Human names only in rows; revision identity lives in the per-source
 * editor's advanced disclosure, never in the primary path.
 */
export function SourceNavigator(props: SourceNavigatorProps) {
  return (
    <Show
      when={props.sources.length > 0}
    >
      <RowList
        label={t("composition.studioData")}
        labelledBy={props.headingId}
      >
        <For each={props.sources}>
          {(source) => (
            <div
              ref={(row) => props.registerRow(source.draftId, row)}
            >
              <RowListItem
                main={
                  <RowListButton
                    ariaLabel={source.name}
                    title={source.name}
                    primary={
                      <span class="rowListName">
                        <UiIcon name={sourceKindIcon(source)} />
                        <span>{source.name}</span>
                      </span>
                    }
                    secondary={sourceKindLabel(source)}
                    selected={props.selectedId === source.draftId}
                    onActivate={() => props.onSelect(source.draftId)}
                  />
                }
              />
            </div>
          )}
        </For>
      </RowList>
    </Show>
  );
}
