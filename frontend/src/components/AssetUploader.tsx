import { createSignal, Show, For } from "solid-js";
import type { Asset } from "~/lib/types";

export interface AssetUploaderProps {
	onUpload: (file: File) => Promise<Asset>;
	onRemove?: (assetId: string) => void;
	assets?: Asset[];
	accept?: string;
}

/**
 * AssetUploader component for uploading and managing entry assets.
 */
export function AssetUploader(props: AssetUploaderProps) {
	const [uploading, setUploading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);
	let fileInputRef: HTMLInputElement | undefined;

	const handleFileChange = async (e: Event) => {
		const input = e.currentTarget as HTMLInputElement;
		const file = input.files?.[0];

		if (!file) return;

		setUploading(true);
		setError(null);

		try {
			await props.onUpload(file);
			// Clear input after successful upload
			if (fileInputRef) {
				fileInputRef.value = "";
			}
		} catch (err) {
			setError(err instanceof Error ? err.message : "Upload failed");
		} finally {
			setUploading(false);
		}
	};

	const handleRemove = (assetId: string) => {
		if (props.onRemove) {
			props.onRemove(assetId);
		}
	};

	const getFileIcon = (filename: string): string => {
		const ext = filename.split(".").pop()?.toLowerCase();
		switch (ext) {
			case "pdf":
				return "📄";
			case "png":
			case "jpg":
			case "jpeg":
			case "gif":
				return "🖼️";
			case "mp3":
			case "m4a":
			case "wav":
				return "🎵";
			case "mp4":
			case "mov":
				return "🎬";
			default:
				return "📎";
		}
	};

	return (
		<div class="asset-uploader">
			{/* Upload Button */}
			<div class="mb-4">
				<label
					for="file-upload"
					class="inline-flex items-center px-4 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 cursor-pointer transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
					classList={{ "opacity-50 cursor-not-allowed": uploading() }}
				>
					<svg class="w-5 h-5 mr-2" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M15.172 7l-6.586 6.586a2 2 0 102.828 2.828l6.414-6.586a4 4 0 00-5.656-5.656l-6.415 6.585a6 6 0 108.486 8.486L20.5 13"
						/>
					</svg>
					<Show when={uploading()} fallback="Upload Asset">
						Uploading...
					</Show>
				</label>
				<input
					ref={fileInputRef}
					id="file-upload"
					type="file"
					class="sr-only"
					aria-label="Upload asset"
					accept={props.accept}
					onChange={handleFileChange}
					disabled={uploading()}
				/>
			</div>

			{/* Error Message */}
			<Show when={error()}>
				<div class="mb-4 p-3 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm">
					{error()}
				</div>
			</Show>

			{/* Assets List */}
			<Show when={props.assets && props.assets.length > 0}>
				<div class="space-y-2">
					<h4 class="text-sm font-medium text-gray-700">Assets</h4>
					<For each={props.assets}>
						{(asset) => (
							<div class="flex items-center justify-between p-3 bg-gray-50 rounded-lg hover:bg-gray-100 transition-colors">
								<div class="flex items-center gap-3 flex-1 min-w-0">
									<span class="text-2xl flex-shrink-0">{getFileIcon(asset.name)}</span>
									<div class="flex-1 min-w-0">
										<p class="text-sm font-medium text-gray-900 truncate">{asset.name}</p>
										<p class="text-xs text-gray-500">{asset.path}</p>
									</div>
								</div>
								<Show when={props.onRemove}>
									<button
										type="button"
										onClick={() => handleRemove(asset.id)}
										aria-label={`Remove asset ${asset.name}`}
										class="flex-shrink-0 p-1 text-gray-400 hover:text-red-600 hover:bg-red-50 rounded transition-colors"
									>
										<svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
											<path
												stroke-linecap="round"
												stroke-linejoin="round"
												stroke-width="2"
												d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
											/>
										</svg>
									</button>
								</Show>
							</div>
						)}
					</For>
				</div>
			</Show>
		</div>
	);
}
