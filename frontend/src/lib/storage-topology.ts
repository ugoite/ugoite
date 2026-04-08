import { t } from "~/lib/i18n";
import type { Space } from "~/lib/types";

export type StorageTopologySummary = {
	label: string;
	description: string;
	uri: string | null;
};

const normalizeStorageValue = (value: unknown): string | null => {
	if (typeof value !== "string") return null;
	const normalized = value.trim();
	return normalized ? normalized : null;
};

const readEffectiveStorage = (
	space: Space,
): {
	root: string | null;
	type: string | null;
} => {
	const storage = space.storage;
	const type = normalizeStorageValue(storage?.type)?.toLowerCase() ?? null;
	const root = normalizeStorageValue(storage?.root);
	return { root, type };
};

const formatStorageUri = (storageType: string, root: string): string => {
	if (/^[a-z][a-z0-9+.-]*:\/\//i.test(root)) {
		return root;
	}

	if (storageType === "local" || storageType === "file" || storageType === "fs") {
		return root.startsWith("/") ? `file://${root}` : root;
	}

	return `${storageType}://${root.replace(/^\/+/, "")}`;
};

export const summarizeSpaceStorage = (space: Space): StorageTopologySummary => {
	const { root, type } = readEffectiveStorage(space);
	if (!root || !type) {
		return {
			label: t("storageSummary.backendApi.label"),
			description: t("storageSummary.backendApi.description"),
			uri: null,
		};
	}

	const uri = formatStorageUri(type, root);

	if (type === "local" || type === "file" || type === "fs") {
		return {
			label: t("storageSummary.localFilesystem.label"),
			description: t("storageSummary.localFilesystem.description"),
			uri,
		};
	}

	if (type === "s3") {
		return {
			label: t("storageSummary.remoteObjectStore.label"),
			description: t("storageSummary.remoteObjectStore.description"),
			uri,
		};
	}

	return {
		label: t("storageSummary.backendApi.label"),
		description: t("storageSummary.backendApi.description"),
		uri,
	};
};
