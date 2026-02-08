import { For, Show, createSignal } from "solid-js";
import type { Space } from "~/lib/types";

export interface SpaceSelectorProps {
	spaces: Space[];
	selectedSpaceId: string | null;
	loading: boolean;
	error: string | null;
	onSelect: (spaceId: string) => void;
	onCreate: (name: string) => void;
}

export function SpaceSelector(props: SpaceSelectorProps) {
	const [showCreateForm, setShowCreateForm] = createSignal(false);
	const [newSpaceName, setNewSpaceName] = createSignal("");

	const handleCreate = () => {
		const name = newSpaceName().trim();
		if (name) {
			props.onCreate(name);
			setNewSpaceName("");
			setShowCreateForm(false);
		}
	};

	const handleKeyDown = (e: KeyboardEvent) => {
		if (e.key === "Enter") {
			handleCreate();
		} else if (e.key === "Escape") {
			setShowCreateForm(false);
			setNewSpaceName("");
		}
	};

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
					<button
						type="button"
						class="ui-button ui-button-secondary ui-button-sm shrink-0"
						title="Create new space"
						onClick={() => setShowCreateForm(true)}
					>
						<svg
							class="w-4 h-4"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
							aria-hidden="true"
						>
							<title>Add space</title>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="2"
								d="M12 4v16m8-8H4"
							/>
						</svg>
					</button>
				</Show>
			</div>

			<Show when={props.error}>
				<p class="ui-alert ui-alert-error text-xs mt-2">{props.error}</p>
			</Show>

			<Show when={showCreateForm()}>
				<div class="mt-2 flex gap-2">
					<input
						type="text"
						class="ui-input flex-1 text-sm"
						placeholder="New space name..."
						value={newSpaceName()}
						onInput={(e) => setNewSpaceName(e.currentTarget.value)}
						onKeyDown={handleKeyDown}
					/>
					<button type="button" class="ui-button ui-button-primary text-sm" onClick={handleCreate}>
						Create
					</button>
					<button
						type="button"
						class="ui-button ui-button-secondary text-sm"
						onClick={() => {
							setShowCreateForm(false);
							setNewSpaceName("");
						}}
					>
						Cancel
					</button>
				</div>
			</Show>
		</div>
	);
}
