import { For } from "solid-js";
import { RowList, RowListItem, RowListLink } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import { formatDateLabel } from "~/lib/date-format";
import { t } from "~/lib/i18n";
import {
  compositionDisplayName,
  type CompositionListItem,
} from "~/lib/composition-api";
import { spaceCompositionRevisionPath } from "~/lib/space-path";

interface CompositionRowsProps {
  spaceId: string;
  items: readonly CompositionListItem[];
  label: string;
  labelledBy?: string;
}

export function CompositionRows(props: CompositionRowsProps) {
  return (
    <RowList label={props.label} labelledBy={props.labelledBy}>
      <For each={props.items}>
        {(item) => (
          <RowListItem
            main={
              <RowListLink
                href={spaceCompositionRevisionPath(
                  props.spaceId,
                  item.composition_id,
                  item.revision_id,
                )}
                primary={
                  <span class="rowListName">
                    <UiIcon name="columns" />
                    <span>{compositionDisplayName(item.name)}</span>
                  </span>
                }
                secondary={t("composition.kind.dashboard")}
                meta={formatDateLabel(item.updated_at)}
                chevron
              />
            }
          />
        )}
      </For>
    </RowList>
  );
}
