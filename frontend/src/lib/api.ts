import {
  getFrontendTestApiBase,
  getRuntimeFrontendApiBase,
} from "./frontend-origin";

export type RuntimeCapabilities = {
  mode: "server-backed";
  serverBacked: true;
  browserLocal: false;
  sync: "none";
};

export const runtimeCapabilities: RuntimeCapabilities = {
  mode: "server-backed",
  serverBacked: true,
  browserLocal: false,
  sync: "none",
};

export const getBackendBase = (): string => {
  // In test environment, use absolute URL for MSW to intercept
  /* v8 ignore start */
  if (typeof process !== "undefined" && process.env?.NODE_ENV === "test") {
    return getFrontendTestApiBase();
  }
  /* v8 ignore stop */
  /* v8 ignore start */
  // In SSR, Node's fetch requires an absolute URL.
  // Default to the frontend dev server origin used in e2e/dev.
  if (typeof window === "undefined") {
    return getRuntimeFrontendApiBase();
  }
  // Always use /api which is proxied to the backend in development
  // and should be served by the backend or a reverse proxy in production.
  return "/api";
  /* v8 ignore stop */
};

export const joinUrl = (base: string, path = "/"): string => {
  if (!base) return path;
  const b = base.replace(/\/$/, "");
  const p = path.replace(/^\//, "");
  return `${b}/${p}`;
};

export type ApiFetchOptions = RequestInit;

const applyServerRequestAuth = async (
  requestInit: RequestInit,
): Promise<RequestInit> => {
  if (typeof window !== "undefined") {
    return requestInit;
  }

  try {
    const { getRequestEvent } = await import("solid-js/web");
    const event = getRequestEvent();
    if (!event) {
      return requestInit;
    }
    const headers = new Headers(requestInit.headers);
    const cookieHeader = event.request.headers.get("cookie");
    if (cookieHeader && !headers.has("cookie")) {
      headers.set("cookie", cookieHeader);
    }
    const authorizationHeader = event.request.headers.get("authorization");
    if (authorizationHeader && !headers.has("authorization")) {
      headers.set("authorization", authorizationHeader);
    }
    return {
      ...requestInit,
      headers,
    };
  } catch {
    return requestInit;
  }
};

export const apiFetch = async (path = "/", options?: ApiFetchOptions) => {
  const base = getBackendBase();
  let url: string;
  /* v8 ignore start */
  if (/^https?:\/\//.test(base)) {
    url = `${base.replace(/\/$/, "")}${
      path.startsWith("/") ? path : `/${path}`
    }`;
  } else {
    // relative path; base probably like '/api'
    url = `${base}${path.startsWith("/") ? path : `/${path}`}`;
  }
  /* v8 ignore stop */
  // Quiet loading: local panel/row refetches never unmount the shell.
  // Callers render a panel-local `LocalBusyIndicator` while rows stay
  // visible; route/auth/bootstrap pending UI covers first paint only.
  const serverAwareRequestInit = await applyServerRequestAuth(options ?? {});
  return await fetch(url, serverAwareRequestInit);
};
