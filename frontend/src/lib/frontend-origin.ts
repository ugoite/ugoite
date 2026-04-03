const defaultFrontendOrigin = "http://localhost:3000";

const normalizeOrigin = (origin: string): string => origin.replace(/\/$/, "");

const readEnvValue = (
	env: Record<string, string | undefined>,
	keys: readonly string[],
): string | undefined => {
	for (const key of keys) {
		const value = env[key]?.trim();
		if (value) {
			return value;
		}
	}
	return undefined;
};

const getEnvMap = (): Record<string, string | undefined> =>
	typeof process !== "undefined" && process.env
		? (process.env as Record<string, string | undefined>)
		: {};

export const getFrontendTestOrigin = (env = getEnvMap()): string =>
	normalizeOrigin(readEnvValue(env, ["FRONTEND_TEST_ORIGIN"]) ?? defaultFrontendOrigin);

export const getRuntimeFrontendOrigin = (env = getEnvMap()): string =>
	normalizeOrigin(
		readEnvValue(env, ["FRONTEND_ORIGIN", "ORIGIN", "FRONTEND_URL"]) ?? defaultFrontendOrigin,
	);

export const getFrontendApiBase = (origin: string): string => `${normalizeOrigin(origin)}/api`;

export const getFrontendTestApiBase = (env = getEnvMap()): string =>
	getFrontendApiBase(getFrontendTestOrigin(env));

export const getRuntimeFrontendApiBase = (env = getEnvMap()): string =>
	getFrontendApiBase(getRuntimeFrontendOrigin(env));
