import { For, Show } from "solid-js";
import type { Space } from "~/lib/types";
import { t } from "~/lib/i18n";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { spaceUid } from "~/lib/space-list";

export interface SpaceSelectorProps {
  spaces: Space[];
  selectedSpaceId: string | null;
  loading: boolean;
  error: string | null;
  onSelect: (spaceId: string) => void;
}

export function SpaceSelector(props: SpaceSelectorProps) {
  return (
    <div class="ui-toolbar">
      <div class="flex items-center gap-2">
        <label for="space-select" class="ui-label text-xs shrink-0">
          {t("common.space")}:
        </label>
        {/* Spinner alongside the selector: options stay mounted. */}
        <Show when={props.loading}>
          <LocalBusyIndicator size="sm" label={t("common.loading")} />
        </Show>
        <select
          id="space-select"
          class="ui-input min-w-0 flex-1 text-sm truncate"
          value={props.selectedSpaceId || ""}
          onChange={(e) => props.onSelect(e.currentTarget.value)}
        >
          <For each={props.spaces}>
            {(space) => (
              <option value={spaceUid(space)}>
                {space.name || space.slug || spaceUid(space)}
              </option>
            )}
          </For>
        </select>
      </div>

      <Show when={props.error}>
        <p class="ui-alert ui-alert-error text-xs mt-2">{props.error}</p>
      </Show>
    </div>
  );

  /* v8 ignore start */
}
/* v8 ignore stop */
