// REQ-SEC-003: authenticated SSR request forwarding.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";

const getRequestEventMock = vi.fn();
const authConfig = {
  status: "active",
  node_id: "01900000-0000-7000-8000-000000000001",
  issuer: "http://localhost:3000",
  rp_id: "localhost",
  passkey: true,
  oidc: false,
  login_required: true,
};

vi.mock("solid-js/web", () => ({
  getRequestEvent: getRequestEventMock,
}));

describe("apiFetch auth forwarding", () => {
  beforeEach(() => {
    getRequestEventMock.mockReset();
    vi.resetModules();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  it("REQ-OPS-015: forwards SSR auth headers to local auth requests", async () => {
    let seenCookie: string | null = null;
    let seenAuthorization: string | null = null;
    server.use(
      http.get(testApiUrl("/auth/config"), ({ request }) => {
        seenCookie = request.headers.get("cookie");
        seenAuthorization = request.headers.get("authorization");
        return HttpResponse.json(authConfig);
      }),
    );
    vi.stubGlobal("window", undefined);
    getRequestEventMock.mockReturnValue({
      request: new Request("http://localhost:3000/login", {
        headers: {
          cookie: "ugoite_session=server-session",
          authorization: "DPoP forwarded-token",
        },
      }),
    });

    const { apiFetch } = await import("./api");
    const response = await apiFetch("/auth/config", { trackLoading: false });

    expect(response.status).toBe(200);
    expect(seenCookie).toBe("ugoite_session=server-session");
    expect(seenAuthorization).toBe("DPoP forwarded-token");
  });

  it("exposes the current server-backed runtime capabilities", async () => {
    const { runtimeCapabilities } = await import("./ugoite-client");

    expect(runtimeCapabilities).toEqual({
      mode: "server-backed",
      serverBacked: true,
      browserLocal: false,
      sync: "none",
    });
  });

  it("REQ-OPS-015: preserves explicit request auth headers during SSR", async () => {
    let seenCookie: string | null = null;
    let seenAuthorization: string | null = null;
    server.use(
      http.get(testApiUrl("/auth/config"), ({ request }) => {
        seenCookie = request.headers.get("cookie");
        seenAuthorization = request.headers.get("authorization");
        return HttpResponse.json(authConfig);
      }),
    );
    vi.stubGlobal("window", undefined);
    getRequestEventMock.mockReturnValue({
      request: new Request("http://localhost:3000/login", {
        headers: {
          cookie: "ugoite_session=server-session",
          authorization: "DPoP forwarded-token",
        },
      }),
    });

    const { apiFetch } = await import("./api");
    const response = await apiFetch("/auth/config", {
      trackLoading: false,
      headers: {
        cookie: "ugoite_session=explicit-session",
        authorization: "DPoP explicit-token",
      },
    });

    expect(response.status).toBe(200);
    expect(seenCookie).toBe("ugoite_session=explicit-session");
    expect(seenAuthorization).toBe("DPoP explicit-token");
  });

  it("REQ-OPS-015: skips SSR auth forwarding without a request event", async () => {
    let seenCookie: string | null = "unexpected";
    let seenAuthorization: string | null = "unexpected";
    server.use(
      http.get(testApiUrl("/auth/config"), ({ request }) => {
        seenCookie = request.headers.get("cookie");
        seenAuthorization = request.headers.get("authorization");
        return HttpResponse.json(authConfig);
      }),
    );
    vi.stubGlobal("window", undefined);
    getRequestEventMock.mockReturnValue(undefined);

    const { apiFetch } = await import("./api");
    const response = await apiFetch("/auth/config", { trackLoading: false });

    expect(response.status).toBe(200);
    expect(seenCookie).toBeNull();
    expect(seenAuthorization).toBeNull();
  });

  it("REQ-OPS-015: uses FRONTEND_URL for the SSR API origin when frontend origin env vars are unset", async () => {
    let seenOrigin: string | null = null;
    let seenCookie: string | null = null;
    server.use(
      http.get("http://localhost:13000/api/auth/config", ({ request }) => {
        seenOrigin = new URL(request.url).origin;
        seenCookie = request.headers.get("cookie");
        return HttpResponse.json(authConfig);
      }),
    );
    vi.stubGlobal("window", undefined);
    vi.stubEnv("NODE_ENV", "development");
    vi.stubEnv("FRONTEND_ORIGIN", "");
    vi.stubEnv("ORIGIN", "");
    vi.stubEnv("FRONTEND_URL", "http://localhost:13000");
    getRequestEventMock.mockReturnValue({
      request: new Request("http://attacker.invalid/login", {
        headers: {
          cookie: "ugoite_session=server-session",
        },
      }),
    });

    const { apiFetch } = await import("./api");
    const response = await apiFetch("/auth/config", { trackLoading: false });

    expect(response.status).toBe(200);
    expect(seenOrigin).toBe("http://localhost:13000");
    expect(seenCookie).toBe("ugoite_session=server-session");
  });

  it("REQ-OPS-015: uses FRONTEND_TEST_ORIGIN for the frontend test API base", async () => {
    let seenOrigin: string | null = null;
    vi.stubEnv("NODE_ENV", "test");
    vi.stubEnv("FRONTEND_TEST_ORIGIN", "http://127.0.0.1:4310");
    server.use(
      http.get(testApiUrl("/auth/config"), ({ request }) => {
        seenOrigin = new URL(request.url).origin;
        return HttpResponse.json(authConfig);
      }),
    );

    const { apiFetch, getBackendBase } = await import("./api");
    const response = await apiFetch("/auth/config", { trackLoading: false });

    expect(getBackendBase()).toBe("http://127.0.0.1:4310/api");
    expect(response.status).toBe(200);
    expect(seenOrigin).toBe("http://127.0.0.1:4310");
  });

  it("REQ-OPS-015: falls back to the default SSR origin when no frontend origin env is configured", async () => {
    vi.stubGlobal("window", undefined);
    vi.stubEnv("NODE_ENV", "development");
    vi.stubEnv("FRONTEND_ORIGIN", "");
    vi.stubEnv("ORIGIN", "");
    vi.stubEnv("FRONTEND_URL", "");
    getRequestEventMock.mockReturnValue(undefined);

    const { getBackendBase } = await import("./api");

    expect(getBackendBase()).toBe("http://localhost:3000/api");
  });

  it("REQ-OPS-015: does not derive the SSR API origin from the incoming request", async () => {
    vi.stubGlobal("window", undefined);
    vi.stubEnv("NODE_ENV", "development");
    vi.stubEnv("FRONTEND_ORIGIN", "");
    vi.stubEnv("ORIGIN", "");
    vi.stubEnv("FRONTEND_URL", "");
    getRequestEventMock.mockReturnValue({
      request: new Request("http://attacker.invalid/login"),
    });

    const { getBackendBase } = await import("./api");

    expect(getBackendBase()).toBe("http://localhost:3000/api");
  });
});
