import { For } from "solid-js";
import type { Form } from "~/lib/types";

interface FormListProps {
	entryForms: Form[];
	selectedForm: Form | null;
	onSelect: (entryForm: Form) => void;
}

export function FormList(props: FormListProps) {
	return (
		<div class="w-64 border-r border-gray-200 dark:border-gray-800 h-full overflow-y-auto bg-gray-50 dark:bg-gray-900 flex-shrink-0">
			<div class="p-4 border-b border-gray-200 dark:border-gray-800">
				<h2 class="text-sm font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
					Forms
				</h2>
			</div>
			<ul>
				<For each={props.entryForms}>
					{(entryForm) => (
						<li>
							<button
								type="button"
								onClick={() => props.onSelect(entryForm)}
								class={`w-full text-left px-4 py-3 text-sm hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors ${
									props.selectedForm?.name === entryForm.name
										? "bg-white dark:bg-gray-800 border-l-4 border-blue-500 shadow-sm"
										: "border-l-4 border-transparent"
								}`}
							>
								<div class="font-medium text-gray-900 dark:text-gray-100">{entryForm.name}</div>
								<div class="text-xs text-gray-500 mt-1">
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
