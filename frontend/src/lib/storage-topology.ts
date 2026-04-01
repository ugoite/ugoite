import { t } from "~/lib/i18n";
import type { Space } from "~/lib/types";

export type StorageTopologySummary = {
	label: string;
	description: string;
	uri: string | null;
};

const readStorageUri = (space: Space): string | null => {
	const value = space.storage_config?.uri;
	if (typeof value !== "string") return null;
	const uri = value.trim();
	return uri ? uri : null;
};

export const summarizeSpaceStorage = (space: Space): StorageTopologySummary => {
	const uri = readStorageUri(space);
	if (!uri) {
		return {
			label: t("storageSummary.backendApi.label"),
			description: t("storageSummary.backendApi.description"),
			uri: null,
		};
	}

	if (/^(file|fs):\/\//i.test(uri)) {
		return {
			label: t("storageSummary.localFilesystem.label"),
			description: t("storageSummary.localFilesystem.description"),
			uri,
		};
	}

	if (/^s3:\/\//i.test(uri)) {
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
