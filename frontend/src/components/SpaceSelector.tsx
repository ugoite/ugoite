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
		<div class="border-b bg-gray-50 px-4 py-2">
			<div class="flex items-center gap-2">
				<label for="space-select" class="text-xs text-gray-500 font-medium shrink-0">
					Space:
				</label>
				<Show when={props.loading}>
					<span class="text-xs text-gray-400">Loading...</span>
				</Show>
				<Show when={!props.loading}>
					<select
						id="space-select"
						class="min-w-0 flex-1 text-sm border border-gray-200 rounded px-2 py-1 bg-white focus:outline-none focus:ring-1 focus:ring-blue-500 truncate"
						value={props.selectedSpaceId || ""}
						onChange={(e) => props.onSelect(e.currentTarget.value)}
					>
						<For each={props.spaces}>
							{(space) => <option value={space.id}>{space.name || space.id}</option>}
						</For>
					</select>
					<button
						type="button"
						class="p-1 text-gray-500 hover:text-gray-700 hover:bg-gray-100 rounded shrink-0"
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
				<p class="text-xs text-red-500 mt-1">{props.error}</p>
			</Show>

			<Show when={showCreateForm()}>
				<div class="mt-2 flex gap-2">
					<input
						type="text"
						class="flex-1 text-sm border border-gray-300 rounded px-2 py-1 focus:outline-none focus:ring-1 focus:ring-blue-500"
						placeholder="New space name..."
						value={newSpaceName()}
						onInput={(e) => setNewSpaceName(e.currentTarget.value)}
						onKeyDown={handleKeyDown}
					/>
					<button
						type="button"
						class="px-2 py-1 text-sm bg-blue-500 text-white rounded hover:bg-blue-600"
						onClick={handleCreate}
					>
						Create
					</button>
					<button
						type="button"
						class="px-2 py-1 text-sm text-gray-500 hover:text-gray-700"
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
