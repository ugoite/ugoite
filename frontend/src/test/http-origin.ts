import { getFrontendTestApiBase, getFrontendTestOrigin } from "~/lib/frontend-origin";

export const getConfiguredFrontendTestOrigin = (): string => getFrontendTestOrigin();

export const getConfiguredFrontendTestApiBase = (): string => getFrontendTestApiBase();

const normalizeTestPath = (path = "/"): string => (path.startsWith("/") ? path : `/${path}`);

export const testApiPath = (path = "/"): string => `/api${normalizeTestPath(path)}`;

export const testApiUrl = (path = "/"): string =>
	`${getConfiguredFrontendTestApiBase()}${normalizeTestPath(path)}`;

export const testAppUrl = (path = "/"): string =>
	`${getConfiguredFrontendTestOrigin()}${normalizeTestPath(path)}`;
