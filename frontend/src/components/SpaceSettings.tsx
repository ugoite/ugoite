import { createMemo, createSignal, Show } from "solid-js";
import { getDocsiteHref } from "~/lib/docsite-links";
import { t } from "~/lib/i18n";
import { summarizeSpaceStorage } from "~/lib/storage-topology";
import type { Space, SpacePatchPayload } from "~/lib/types";

const storageMigrationGuideUrl = getDocsiteHref(
	"/docs/guide/storage-migration",
	"docs/guide/storage-migration.md",
);

export interface SpaceSettingsProps {
	space: Space;
	onSave: (payload: SpacePatchPayload) => Promise<void>;
	onTestConnection?: (config: Record<string, unknown>) => Promise<{ status: string }>;
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
	const storageSummary = createMemo(() => summarizeSpaceStorage(props.space));

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
			/* v8 ignore start */
			setSaveError(err instanceof Error ? err.message : "Failed to save settings");
			/* v8 ignore stop */
		} finally {
			setIsSaving(false);
		}
	};

	const handleTestConnection = async () => {
		/* v8 ignore start */
		if (!props.onTestConnection) return;
		/* v8 ignore stop */

		setIsTesting(true);
		setTestStatus(null);
		setTestMessage("");

		try {
			const result = await props.onTestConnection({ uri: storageUri() });
			setTestStatus("success");
			setTestMessage(`Connection successful (${result.status})`);
		} catch (err) {
			setTestStatus("error");
			/* v8 ignore start */
			setTestMessage(err instanceof Error ? err.message : "Connection failed");
			/* v8 ignore stop */
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
					<p class="text-sm ui-muted">
						See where this space currently writes data. The saved URI below is migration metadata,
						so updating it does not reroute writes until per-space routing support lands.
					</p>

					<div class="space-y-4">
						<section class="ui-card ui-stack-sm">
							<div class="ui-stack-sm">
								<h4 class="text-lg font-semibold">{t("storageSummary.heading")}</h4>
								<p class="text-sm ui-muted">{storageSummary().description}</p>
							</div>
							<div class="flex flex-wrap items-center gap-2">
								<span class="ui-pill">{storageSummary().label}</span>
							</div>
							{storageSummary().uri ? (
								<div class="ui-stack-sm">
									<p class="text-xs ui-muted">{t("storageSummary.uriLabel")}</p>
									<p class="font-mono text-xs break-all">{storageSummary().uri}</p>
								</div>
							) : null}
						</section>
						<div class="ui-field">
							<label for="storage-uri" class="ui-label">
								Saved Storage URI (metadata only)
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
								Supported planning targets: <code>file://</code> (local), <code>s3://</code> (S3
								bucket)
							</p>
							<div class="ui-card mt-3 space-y-3">
								<div>
									<h4 class="text-sm font-semibold">Plan connector metadata deliberately</h4>
									<ul class="mt-2 list-disc pl-5 text-sm ui-muted space-y-1">
										<li>
											<code>file://</code> records a local path you may want to migrate this space
											to later.
										</li>
										<li>
											<code>s3://</code> records an object-storage target you may want to validate
											or migrate to later.
										</li>
									</ul>
								</div>
								<p class="text-sm ui-muted">
									Changing the saved storage URI only updates this space&apos;s metadata. Ugoite
									keeps writing through the storage root shown above until per-space routing or
									migration support lands.
								</p>
								<p class="text-sm ui-muted">
									Before saving a new URI, review the{" "}
									<a
										href={storageMigrationGuideUrl}
										target="_blank"
										rel="noopener"
										class="hover:underline"
									>
										storage migration guide
									</a>{" "}
									and use Test Connection as a preflight check for the target credentials and
									reachability.
								</p>
							</div>
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

	/* v8 ignore start */
}
/* v8 ignore stop */
