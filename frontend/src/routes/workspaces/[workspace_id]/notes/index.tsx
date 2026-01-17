export default function WorkspaceNotesIndexPane() {
	return (
		<div class="flex-1 flex items-center justify-center text-gray-400">
			<div class="text-center">
				<svg
					class="w-16 h-16 mx-auto mb-4 opacity-50"
					fill="none"
					stroke="currentColor"
					viewBox="0 0 24 24"
					aria-hidden="true"
				>
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="1"
						d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"
					/>
				</svg>
				<p>Select a note to view</p>
			</div>
		</div>
	);
}
