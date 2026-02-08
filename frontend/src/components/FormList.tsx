import { For } from "solid-js";
import type { Form } from "~/lib/types";

interface FormListProps {
	entryForms: Form[];
	selectedForm: Form | null;
	onSelect: (entryForm: Form) => void;
}

export function FormList(props: FormListProps) {
	return (
		<div class="ui-sidebar w-64 h-full overflow-y-auto flex-shrink-0">
			<div class="ui-sidebar-header p-4">
				<h2 class="ui-label text-sm uppercase tracking-wider">Forms</h2>
			</div>
			<ul>
				<For each={props.entryForms}>
					{(entryForm) => (
						<li>
							<button
								type="button"
								onClick={() => props.onSelect(entryForm)}
								class={`ui-list-item w-full text-left px-4 py-3 text-sm ${
									props.selectedForm?.name === entryForm.name ? "ui-list-item-selected" : ""
								}`}
							>
								<div class="font-medium">{entryForm.name}</div>
								<div class="text-xs ui-muted mt-1">
									{Object.keys(entryForm.fields).length} fields
								</div>
							</button>
						</li>
					)}
				</For>
			</ul>
		</div>
	);
}
