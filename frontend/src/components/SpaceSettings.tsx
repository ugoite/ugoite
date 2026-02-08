import { createSignal, Show } from "solid-js";
import type { Space, SpacePatchPayload } from "~/lib/types";

export interface SpaceSettingsProps {
	space: Space;
	onSave: (payload: SpacePatchPayload) => Promise<void>;
	onTestConnection?: (config: { uri: string }) => Promise<{ status: string }>;
}

/**
 * SpaceSettings component for configuring space storage and settings.
 * Implements Story 3: "Bring Your Own Cloud"
 */
export function SpaceSettings(props: SpaceSettingsProps) {
	const [name, setName] = createSignal(props.space.name);
	const [storageUri, setStorageUri] = createSignal(props.space.storage_config?.uri || "");
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
		<div class="space-settings max-w-2xl mx-auto ui-card">
			<h2 class="text-xl font-semibold mb-6">Space Settings</h2>

			<form onSubmit={handleSave} class="space-y-6">
				{/* Space Name */}
				<div class="ui-field">
					<label for="space-name" class="ui-label">
						Space Name
					</label>
					<input
						id="space-name"
						type="text"
						value={name()}
						onInput={(e) => setName(e.currentTarget.value)}
						class="ui-input"
						required
					/>
				</div>

				{/* Storage Configuration */}
				<div class="border-t pt-6">
					<h3 class="text-lg font-semibold mb-4">Storage Configuration</h3>

					<div class="space-y-4">
						<div class="ui-field">
							<label for="storage-uri" class="ui-label">
								Storage URI
							</label>
							<input
								id="storage-uri"
								type="text"
								value={storageUri()}
								onInput={(e) => setStorageUri(e.currentTarget.value)}
								placeholder="file:///local/path or s3://bucket/path"
								class="ui-input"
								required
							/>
							<p class="text-sm ui-muted">
								Supported: <code>file://</code> (local), <code>s3://</code> (S3 bucket)
							</p>
						</div>

						{/* Test Connection Button */}
						<Show when={props.onTestConnection}>
							<button
								type="button"
								onClick={handleTestConnection}
								disabled={isTesting() || !storageUri()}
								class="ui-button ui-button-secondary"
							>
								<Show when={isTesting()} fallback="Test Connection">
									Testing...
								</Show>
							</button>
						</Show>

						{/* Test Status */}
						<Show when={testStatus()}>
							<div
								class="ui-alert"
								classList={{
									"ui-alert-success": testStatus() === "success",
									"ui-alert-error": testStatus() === "error",
								}}
							>
								{testMessage()}
							</div>
						</Show>
					</div>
				</div>

				{/* Save Error */}
				<Show when={saveError()}>
					<div class="ui-alert ui-alert-error">{saveError()}</div>
				</Show>

				{/* Actions */}
				<div class="flex gap-4 border-t pt-6">
					<button type="submit" disabled={isSaving()} class="ui-button ui-button-primary">
						<Show when={isSaving()} fallback="Save Settings">
							Saving...
						</Show>
					</button>
				</div>
			</form>
		</div>
	);
}
