import { createSignal, Show } from "solid-js";
import type { Workspace, WorkspacePatchPayload } from "~/lib/types";

export interface WorkspaceSettingsProps {
	workspace: Workspace;
	onSave: (payload: WorkspacePatchPayload) => Promise<void>;
	onTestConnection?: (config: { uri: string }) => Promise<{ status: string }>;
}

/**
 * WorkspaceSettings component for configuring workspace storage and settings.
 * Implements Story 3: "Bring Your Own Cloud"
 */
export function WorkspaceSettings(props: WorkspaceSettingsProps) {
	const [name, setName] = createSignal(props.workspace.name);
	const [storageUri, setStorageUri] = createSignal(props.workspace.storage_config?.uri || "");
	const [isSaving, setIsSaving] = createSignal(false);
	const [isTesting, setIsTesting] = createSignal(false);
	const [testStatus, setTestStatus] = createSignal<"success" | "error" | null>(null);
	const [testMessage, setTestMessage] = createSignal<string>("");
	const [saveError, setSaveError] = createSignal<string | null>(null);

	const handleSave = async (e: Event) => {
		e.preventDefault();
		setIsSaving(true);
		setSaveError(null);

		try {
			await props.onSave({
				name: name(),
				storage_config: {
					uri: storageUri(),
				},
			});
		} catch (err) {
			setSaveError(err instanceof Error ? err.message : "Failed to save settings");
		} finally {
			setIsSaving(false);
		}
	};

	const handleTestConnection = async () => {
		if (!props.onTestConnection) return;

		setIsTesting(true);
		setTestStatus(null);
		setTestMessage("");

		try {
			const result = await props.onTestConnection({ uri: storageUri() });
			setTestStatus("success");
			setTestMessage(`Connection successful (${result.status})`);
		} catch (err) {
			setTestStatus("error");
			setTestMessage(err instanceof Error ? err.message : "Connection failed");
		} finally {
			setIsTesting(false);
		}
	};

	return (
		<div class="workspace-settings max-w-2xl mx-auto p-6">
			<h2 class="text-2xl font-bold mb-6">Workspace Settings</h2>

			<form onSubmit={handleSave} class="space-y-6">
				{/* Workspace Name */}
				<div>
					<label for="workspace-name" class="block text-sm font-medium text-gray-700 mb-2">
						Workspace Name
					</label>
					<input
						id="workspace-name"
						type="text"
						value={name()}
						onInput={(e) => setName(e.currentTarget.value)}
						class="w-full px-3 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
						required
					/>
				</div>

				{/* Storage Configuration */}
				<div class="border-t pt-6">
					<h3 class="text-lg font-semibold mb-4">Storage Configuration</h3>

					<div class="space-y-4">
						<div>
							<label for="storage-uri" class="block text-sm font-medium text-gray-700 mb-2">
								Storage URI
							</label>
							<input
								id="storage-uri"
								type="text"
								value={storageUri()}
								onInput={(e) => setStorageUri(e.currentTarget.value)}
								placeholder="file:///local/path or s3://bucket/path"
								class="w-full px-3 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
								required
							/>
							<p class="mt-1 text-sm text-gray-500">
								Supported: <code class="bg-gray-100 px-1 rounded">file://</code> (local),{" "}
								<code class="bg-gray-100 px-1 rounded">s3://</code> (S3 bucket)
							</p>
						</div>

						{/* Test Connection Button */}
						<Show when={props.onTestConnection}>
							<button
								type="button"
								onClick={handleTestConnection}
								disabled={isTesting() || !storageUri()}
								class="px-4 py-2 bg-gray-500 text-white rounded-lg hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed"
							>
								<Show when={isTesting()} fallback="Test Connection">
									Testing...
								</Show>
							</button>
						</Show>

						{/* Test Status */}
						<Show when={testStatus()}>
							<div
								class="p-3 rounded-lg"
								classList={{
									"bg-green-50 border border-green-200 text-green-700": testStatus() === "success",
									"bg-red-50 border border-red-200 text-red-700": testStatus() === "error",
								}}
							>
								{testMessage()}
							</div>
						</Show>
					</div>
				</div>

				{/* Save Error */}
				<Show when={saveError()}>
					<div class="p-3 bg-red-50 border border-red-200 rounded-lg text-red-700">
						{saveError()}
					</div>
				</Show>

				{/* Actions */}
				<div class="flex gap-4 border-t pt-6">
					<button
						type="submit"
						disabled={isSaving()}
						class="px-6 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
					>
						<Show when={isSaving()} fallback="Save Settings">
							Saving...
						</Show>
					</button>
				</div>
			</form>
		</div>
	);
}
