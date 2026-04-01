import { t } from "~/lib/i18n";
import type { Space } from "~/lib/types";

export type StorageTopologySummary = {
	label: string;
	description: string;
	uri: string | null;
};

const LOCAL_FILESYSTEM_SCHEMES = ["file", "fs"] as const;
const REMOTE_OBJECT_STORE_SCHEMES = [
	"s3",
	"gcs",
	"gs",
	"azblob",
	"azdls",
	"abfs",
	"abfss",
	"oss",
] as const;

const readStorageUri = (space: Space): string | null => {
	const value = space.storage_config?.uri;
	if (typeof value !== "string") return null;
	const uri = value.trim();
	return uri ? uri : null;
};

const hasKnownScheme = (uri: string, schemes: readonly string[]): boolean => {
	const [scheme = ""] = uri.split("://", 1);
	return schemes.includes(scheme.toLowerCase());
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

	if (hasKnownScheme(uri, LOCAL_FILESYSTEM_SCHEMES)) {
		return {
			label: t("storageSummary.localFilesystem.label"),
			description: t("storageSummary.localFilesystem.description"),
			uri,
		};
	}

	if (hasKnownScheme(uri, REMOTE_OBJECT_STORE_SCHEMES)) {
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
