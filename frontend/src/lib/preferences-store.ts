import { createRoot, createSignal } from "solid-js";
import { initializeLocale, setLocale } from "./i18n";
import { preferencesApi } from "./preferences-api";
import {
	emptyUserPreferences,
	readLocalPreferences,
	writeLocalPreferences,
} from "./preferences-local";
import { initializeUiTheme, setColorMode, setPrimaryColor, setUiTheme } from "./ui-theme";
import type { UserPreferences, UserPreferencesPatchPayload } from "./types";

const mergePreferences = (
	primary: Partial<UserPreferences> | null | undefined,
	fallback: Partial<UserPreferences> | null | undefined,
): UserPreferences => {
	const preferences = emptyUserPreferences();
	preferences.selected_space_id = primary?.selected_space_id ?? fallback?.selected_space_id ?? null;
	preferences.locale = primary?.locale ?? fallback?.locale ?? null;
	preferences.ui_theme = primary?.ui_theme ?? fallback?.ui_theme ?? null;
	preferences.color_mode = primary?.color_mode ?? fallback?.color_mode ?? null;
	preferences.primary_color = primary?.primary_color ?? fallback?.primary_color ?? null;
	return preferences;
};

const applyPatch = (
	current: UserPreferences,
	patch: UserPreferencesPatchPayload,
): UserPreferences => {
	const preferences = emptyUserPreferences();
	preferences.selected_space_id =
		patch.selected_space_id !== undefined ? patch.selected_space_id : current.selected_space_id;
	preferences.locale = patch.locale !== undefined ? patch.locale : current.locale;
	preferences.ui_theme = patch.ui_theme !== undefined ? patch.ui_theme : current.ui_theme;
	preferences.color_mode = patch.color_mode !== undefined ? patch.color_mode : current.color_mode;
	preferences.primary_color =
		patch.primary_color !== undefined ? patch.primary_color : current.primary_color;
	return preferences;
};

const missingPortableFields = (
	portable: UserPreferences,
	local: UserPreferences,
): UserPreferencesPatchPayload => {
	const patch: UserPreferencesPatchPayload = {};
	if (portable.selected_space_id === null && local.selected_space_id !== null) {
		patch.selected_space_id = local.selected_space_id;
	}
	if (portable.locale === null && local.locale !== null) {
		patch.locale = local.locale;
	}
	if (portable.ui_theme === null && local.ui_theme !== null) {
		patch.ui_theme = local.ui_theme;
	}
	if (portable.color_mode === null && local.color_mode !== null) {
		patch.color_mode = local.color_mode;
	}
	if (portable.primary_color === null && local.primary_color !== null) {
		patch.primary_color = local.primary_color;
	}
	return patch;
};

const hasPatchValues = (patch: UserPreferencesPatchPayload): boolean =>
	Object.values(patch).some((value) => value !== undefined);

const normalizePathname = (pathname: string): string => {
	const trimmed = pathname.trim();
	if (!trimmed || trimmed === "/") {
		return "/";
	}
	return trimmed.endsWith("/") ? trimmed.slice(0, -1) : trimmed;
};

const AUTHENTICATED_PORTABLE_PREFERENCES_PREFIXES = ["/entries", "/spaces"] as const;

const isPublicPortablePreferencesPath = (pathname: string): boolean => {
	const normalized = normalizePathname(pathname);
	return !AUTHENTICATED_PORTABLE_PREFERENCES_PREFIXES.some(
		(prefix) => normalized === prefix || normalized.startsWith(`${prefix}/`),
	);
};

const applyUiPreferences = (preferences: UserPreferences): void => {
	if (preferences.locale) {
		setLocale(preferences.locale);
	} else {
		initializeLocale();
	}
	if (preferences.ui_theme) {
		setUiTheme(preferences.ui_theme);
	}
	if (preferences.color_mode) {
		setColorMode(preferences.color_mode);
	}
	if (preferences.primary_color) {
		setPrimaryColor(preferences.primary_color);
	}
	initializeUiTheme();
};

const preferencesStore = createRoot(() => {
	const [portablePreferences, setPortablePreferences] = createSignal<UserPreferences>(
		emptyUserPreferences(),
	);
	const [initialized, setInitialized] = createSignal(false);
	const [loading, setLoading] = createSignal(false);
	let initialization: Promise<UserPreferences> | null = null;

	const syncLocalPreferences = (): UserPreferences => {
		const localPreferences = mergePreferences(readLocalPreferences(), emptyUserPreferences());
		return syncPreferences(localPreferences);
	};

	const syncPreferences = (next: UserPreferences): UserPreferences => {
		setPortablePreferences(next);
		writeLocalPreferences(next);
		applyUiPreferences(next);
		return next;
	};

	const resetPortablePreferencesState = () => {
		initialization = null;
		setInitialized(false);
		setLoading(false);
		setPortablePreferences(readLocalPreferences());
	};

	const initializePortablePreferences = async (): Promise<UserPreferences> => {
		if (initialized()) {
			return portablePreferences();
		}
		if (initialization) {
			return initialization;
		}

		const localPreferences = syncLocalPreferences();
		setLoading(true);

		initialization = (async () => {
			try {
				const remotePreferences = mergePreferences(
					await preferencesApi.getMe(),
					emptyUserPreferences(),
				);
				const effectivePreferences = mergePreferences(remotePreferences, localPreferences);
				syncPreferences(effectivePreferences);

				const migrationPatch = missingPortableFields(remotePreferences, localPreferences);
				if (hasPatchValues(migrationPatch)) {
					const migratedPreferences = mergePreferences(
						await preferencesApi.patchMe(migrationPatch),
						effectivePreferences,
					);
					syncPreferences(migratedPreferences);
				}
				return portablePreferences();
			} catch {
				/* v8 ignore start */
				return syncPreferences(localPreferences);
				/* v8 ignore stop */
			} finally {
				setInitialized(true);
				setLoading(false);
				initialization = null;
			}
		})();

		return initialization;
	};

	const initializePortablePreferencesForPath = async (
		pathname: string,
	): Promise<UserPreferences> => {
		if (initialized() || initialization) {
			return initializePortablePreferences();
		}
		if (isPublicPortablePreferencesPath(pathname)) {
			return syncLocalPreferences();
		}
		return initializePortablePreferences();
	};

	const patchPortablePreferences = async (
		patch: UserPreferencesPatchPayload,
	): Promise<UserPreferences> => {
		if (!hasPatchValues(patch)) {
			return portablePreferences();
		}

		const nextPreferences = syncPreferences(applyPatch(portablePreferences(), patch));
		try {
			return syncPreferences(
				mergePreferences(await preferencesApi.patchMe(patch), nextPreferences),
			);
		} catch {
			/* v8 ignore start */
			return nextPreferences;
			/* v8 ignore stop */
		}
	};

	return {
		portablePreferences,
		initialized,
		loading,
		resetPortablePreferencesState,
		initializePortablePreferences,
		initializePortablePreferencesForPath,
		patchPortablePreferences,
		setSelectedSpacePreference: (selectedSpaceId: string | null) => {
			const patch = {} as UserPreferencesPatchPayload;
			patch.selected_space_id = selectedSpaceId;
			return patchPortablePreferences(patch);
		},
		setLocalePreference: (locale: UserPreferences["locale"]) => {
			const patch = {} as UserPreferencesPatchPayload;
			patch.locale = locale;
			return patchPortablePreferences(patch);
		},
		setUiThemePreference: (uiTheme: UserPreferences["ui_theme"]) => {
			const patch = {} as UserPreferencesPatchPayload;
			patch.ui_theme = uiTheme;
			return patchPortablePreferences(patch);
		},
		setColorModePreference: (colorMode: UserPreferences["color_mode"]) => {
			const patch = {} as UserPreferencesPatchPayload;
			patch.color_mode = colorMode;
			return patchPortablePreferences(patch);
		},
		setPrimaryColorPreference: (primaryColor: UserPreferences["primary_color"]) => {
			const patch = {} as UserPreferencesPatchPayload;
			patch.primary_color = primaryColor;
			return patchPortablePreferences(patch);
		},
	};
});

export const portablePreferences = preferencesStore.portablePreferences;
export const portablePreferencesInitialized = preferencesStore.initialized;
export const portablePreferencesLoading = preferencesStore.loading;
export const resetPortablePreferencesState = preferencesStore.resetPortablePreferencesState;
export const initializePortablePreferences = preferencesStore.initializePortablePreferences;
export const initializePortablePreferencesForPath =
	preferencesStore.initializePortablePreferencesForPath;
export const patchPortablePreferences = preferencesStore.patchPortablePreferences;
export const setSelectedSpacePreference = preferencesStore.setSelectedSpacePreference;
export const setLocalePreference = preferencesStore.setLocalePreference;
export const setUiThemePreference = preferencesStore.setUiThemePreference;
export const setColorModePreference = preferencesStore.setColorModePreference;
export const setPrimaryColorPreference = preferencesStore.setPrimaryColorPreference;
export const isPublicPortablePreferencesRoute = isPublicPortablePreferencesPath;
