import { For, Show } from "solid-js";
import type { Space } from "~/lib/types";

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
					Space:
				</label>
				<Show when={props.loading}>
					<span class="text-xs ui-muted">Loading...</span>
				</Show>
				<Show when={!props.loading}>
					<select
						id="space-select"
						class="ui-input min-w-0 flex-1 text-sm truncate"
						value={props.selectedSpaceId || ""}
						onChange={(e) => props.onSelect(e.currentTarget.value)}
					>
						<For each={props.spaces}>
							{(space) => <option value={space.id}>{space.name || space.id}</option>}
						</For>
					</select>
				</Show>
			</div>

			<Show when={props.error}>
				<p class="ui-alert ui-alert-error text-xs mt-2">{props.error}</p>
			</Show>
		</div>
	);
}
