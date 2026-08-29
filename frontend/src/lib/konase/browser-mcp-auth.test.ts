import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  authorizeBrowserMcp,
  discoverBrowserMcpTarget,
} from "./browser-mcp-auth";

const origin = location.origin;

const response = (value: unknown, status = 200) =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });

const scriptedFetcher = (responses: Response[]) => {
  const calls: Array<{ input: string; init?: RequestInit }> = [];
  const fetcher: typeof fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    const next = responses.shift();
    if (!next) {
      throw new Error("scripted browser MCP fetch ran out of responses");
    }
    return next;
  };
  return { calls, fetcher };
};

const protectedResource = (base = origin) => ({
  resource: `${base}/mcp`,
  authorization_servers: [base],
});

const authorizationMetadata = () => ({
  device_authorization_endpoint: `${origin}/api/oauth/device/authorization`,
  token_endpoint: `${origin}/api/oauth/token`,
});

describe("browser MCP authorization", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("resolves the integrated server's canonical /mcp endpoint after the API proxy fallback", async () => {
    const { calls, fetcher } = scriptedFetcher([
      response({ detail: "not found" }, 404),
      response(protectedResource()),
      response(authorizationMetadata()),
    ]);

    await expect(discoverBrowserMcpTarget(fetcher)).resolves.toEqual({
      resource: `${origin}/mcp`,
      endpoint: "/mcp",
      deviceAuthorizationEndpoint: `${origin}/api/oauth/device/authorization`,
      tokenEndpoint: `${origin}/api/oauth/token`,
    });
    expect(calls.map((call) => call.input)).toEqual([
      "/api/.well-known/oauth-protected-resource",
      "/.well-known/oauth-protected-resource",
      "/.well-known/oauth-authorization-server",
    ]);
  });

  it("requests and accepts a credential for exactly the current Space", async () => {
    const { calls, fetcher } = scriptedFetcher([
      response(protectedResource()),
      response(authorizationMetadata()),
      response({
        device_code: "device-code",
        user_code: "ABCD-EFGH",
        verification_uri_complete: `${origin}/device?user_code=ABCD-EFGH`,
        expires_in: 600,
        interval: 1,
      }, 201),
      response({
        access_token: "space-a-token",
        token_type: "Bearer",
        expires_in: 300,
        space_uid: "space-a-uid",
      }),
    ]);
    const approval = vi.fn();

    await expect(authorizeBrowserMcp({
      spaceUid: "space-a-uid",
      fetcher,
      onApprovalRequired: approval,
    })).resolves.toEqual({
      accessToken: "space-a-token",
      endpoint: "/api/mcp",
      resource: `${origin}/mcp`,
      spaceUid: "space-a-uid",
    });

    expect(approval).toHaveBeenCalledWith({
      verificationUriComplete: `${origin}/device?user_code=ABCD-EFGH`,
      userCode: "ABCD-EFGH",
    });
    const deviceBody = JSON.parse(String(calls[2].init?.body)) as Record<
      string,
      unknown
    >;
    expect(deviceBody).toMatchObject({
      space_uid: "space-a-uid",
      resource: `${origin}/mcp`,
      requested_actions: ["read", "create", "update"],
    });
    const tokenBody = JSON.parse(String(calls[3].init?.body)) as Record<
      string,
      unknown
    >;
    expect(tokenBody).toMatchObject({
      grant_type: "urn:ietf:params:oauth:grant-type:device_code",
      device_code: "device-code",
      resource: `${origin}/mcp`,
    });
    expect(String(tokenBody.client_assertion).split(".")).toHaveLength(3);
    expect(calls[2].init?.credentials).toBe("same-origin");
    expect(calls[3].init?.credentials).toBe("same-origin");
    expect(calls.map((call) => call.input).slice(0, 3)).toEqual([
      "/api/.well-known/oauth-protected-resource",
      "/api/.well-known/oauth-authorization-server",
      `${origin}/api/oauth/device/authorization`,
    ]);
  });

  it("rejects an approved token when the server returns another Space", async () => {
    const { fetcher } = scriptedFetcher([
      response(protectedResource()),
      response(authorizationMetadata()),
      response({
        device_code: "device-code",
        user_code: "ABCD-EFGH",
        verification_uri_complete: `${origin}/device?user_code=ABCD-EFGH`,
      }, 201),
      response({
        access_token: "space-b-token",
        space_uid: "space-b-uid",
      }),
    ]);

    await expect(authorizeBrowserMcp({
      spaceUid: "space-a-uid",
      fetcher,
    })).rejects.toThrow("different Space");
  });

  it("fails closed when protected-resource metadata points to another origin", async () => {
    const { fetcher } = scriptedFetcher([
      response(protectedResource("https://evil.example")),
      response(protectedResource("https://evil.example")),
    ]);

    await expect(discoverBrowserMcpTarget(fetcher)).rejects.toThrow(
      "current browser origin",
    );
  });
});
