import type { APIEvent } from "@solidjs/start/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const makeEvent = (url: string, init: RequestInit = {}): APIEvent =>
  ({ request: new Request(url, init) }) as APIEvent;

describe("api proxy route", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.restoreAllMocks();
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  it("REQ-SEC-003: rejects protocol-relative proxy paths before forwarding browser bearer tokens", async () => {
    vi.stubEnv("BACKEND_URL", "http://127.0.0.1:8000");
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const { GET } = await import("./[...path]");

    const response = await GET(
      makeEvent("http://127.0.0.1:3000/api//127.0.0.1:9998/browser-steal?z=1", {
        headers: {
          cookie: "ugoite_session=browser-session",
        },
      }),
    );

    expect(response.status).toBe(400);
    await expect(response.text()).resolves.toBe("Invalid API proxy path");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("REQ-SEC-003: pins forwarded browser auth to the configured backend origin", async () => {
    vi.stubEnv("BACKEND_URL", "http://127.0.0.1:8000");
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const { GET } = await import("./[...path]");

    const response = await GET(
      makeEvent("http://127.0.0.1:3000/api/spaces?z=1", {
        headers: {
          cookie: "ugoite_session=browser-session",
        },
      }),
    );

    expect(response.status).toBe(200);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [targetUrl, init] = fetchMock.mock.calls[0] as [URL, RequestInit];
    expect(targetUrl.toString()).toBe("http://127.0.0.1:8000/spaces?z=1");
    expect(targetUrl.origin).toBe("http://127.0.0.1:8000");
    const headers = new Headers(init.headers);
    expect(headers.get("cookie")).toBe("ugoite_session=browser-session");
    expect(headers.get("authorization")).toBeNull();
  });

  it("REQ-SEC-003: surfaces malformed BACKEND_URL as server misconfiguration", async () => {
    vi.stubEnv("BACKEND_URL", "http://[");
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const stderrSpy = vi.spyOn(process.stderr, "write").mockReturnValue(true);
    const { GET } = await import("./[...path]");

    const response = await GET(makeEvent("http://127.0.0.1:3000/api/spaces"));

    expect(response.status).toBe(500);
    await expect(response.text()).resolves.toBe("BACKEND_URL is invalid");
    expect(fetchMock).not.toHaveBeenCalled();
    expect(stderrSpy).toHaveBeenCalledWith(
      expect.stringContaining("API proxy backend misconfiguration"),
    );
  });

  it("forwards the human approval header without forwarding unrelated headers", async () => {
    vi.stubEnv("BACKEND_URL", "http://127.0.0.1:8000");
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const { DELETE } = await import("./[...path]");

    await DELETE(
      makeEvent("http://127.0.0.1:3000/api/spaces/demo/entries/entry-1", {
        headers: {
          "x-ugoite-human-approval": "a".repeat(43),
          "x-secret-test-header": "must-not-forward",
        },
      }),
    );

    const [, init] = fetchMock.mock.calls[0] as [URL, RequestInit];
    const headers = new Headers(init.headers);
    expect(headers.get("x-ugoite-human-approval")).toBe("a".repeat(43));
    expect(headers.get("x-secret-test-header")).toBeNull();
  });
});
