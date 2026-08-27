const publicRoutePaths = new Set([
  "/",
  "/login",
  "/recover",
  "/recover/account",
  "/setup",
  "/about",
  "/spaces/join",
]);

const normalizeRoutePath = (pathname: string): string => {
  const normalized = pathname.toLowerCase().replace(/\/+/g, "/").replace(
    /\/+$/,
    "",
  );
  return normalized || "/";
};

export const isProtectedRoute = (pathname: string): boolean => {
  const normalized = normalizeRoutePath(pathname);
  if (publicRoutePaths.has(normalized)) return false;
  return normalized === "/spaces" ||
    normalized.startsWith("/spaces/") ||
    normalized === "/settings/security" ||
    normalized === "/device";
};

export const isPublicRoute = (pathname: string): boolean =>
  publicRoutePaths.has(normalizeRoutePath(pathname));

export const getCurrentPath = (pathname: string, search = ""): string =>
  `${pathname || "/"}${search}`;

export const buildLoginPath = (pathname: string, search = "") =>
  `/login?next=${encodeURIComponent(getCurrentPath(pathname, search))}`;

const defaultNextPath = "/spaces";

export const getSafeNextPath = (value: unknown): string => {
  if (
    typeof value !== "string" ||
    !value.startsWith("/") ||
    value.startsWith("//")
  ) {
    return defaultNextPath;
  }

  try {
    const parsed = new URL(value, "https://ugoite.invalid");
    if (parsed.origin !== "https://ugoite.invalid") return defaultNextPath;
    return `${parsed.pathname}${parsed.search}`;
  } catch {
    return defaultNextPath;
  }
};

const pendingLoginPathKey = "ugoite.pending-login-path";

export const clearPendingLoginPath = (): void => {
  if (typeof sessionStorage === "undefined") return;

  try {
    sessionStorage.removeItem(pendingLoginPathKey);
  } catch {
    // Storage can be unavailable in privacy-restricted browser contexts.
  }
};

export const rememberPendingLoginPath = (value: unknown): void => {
  if (typeof sessionStorage === "undefined") return;

  try {
    sessionStorage.setItem(pendingLoginPathKey, getSafeNextPath(value));
  } catch {
    // Storage can be unavailable in privacy-restricted browser contexts.
  }
};

export const consumePendingLoginPath = (): string | undefined => {
  if (typeof sessionStorage === "undefined") return undefined;

  try {
    const value = sessionStorage.getItem(pendingLoginPathKey);
    clearPendingLoginPath();
    return value ? getSafeNextPath(value) : undefined;
  } catch {
    return undefined;
  }
};
