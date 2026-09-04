type MockOidcServer = {
  issuer: string;
  close: () => void;
};

const encoder = new TextEncoder();

function base64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(
    /=+$/,
    "",
  );
}

function encodedJson(value: unknown): string {
  return base64Url(encoder.encode(JSON.stringify(value)));
}

function jsonResponse(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function isLoopbackHost(host: string): boolean {
  return host === "localhost" || host === "127.0.0.1";
}

export async function startMockOidcServer(
  subject: string,
): Promise<MockOidcServer> {
  const keyPair = await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["sign", "verify"],
  );
  const exportedJwk = await crypto.subtle.exportKey("jwk", keyPair.publicKey);
  const signingJwk = {
    ...exportedJwk,
    alg: "ES256",
    kid: "e2e-key",
    use: "sig",
  };
  const authorizationCodes = new Map<string, string>();
  let issuer = "";

  const advertisedHost = Deno.env.get("E2E_OIDC_MOCK_HOST")?.trim() ||
    "127.0.0.1";
  const bindHost = isLoopbackHost(advertisedHost) ? "127.0.0.1" : "0.0.0.0";

  const server = Deno.serve(
    { hostname: bindHost, port: 0, onListen() {} },
    async (request: Request) => {
      if (!issuer) throw new Error("mock OIDC issuer is not initialized");
      const url = new URL(request.url);
      if (
        request.method === "GET" &&
        url.pathname === "/.well-known/openid-configuration"
      ) {
        return jsonResponse({
          issuer,
          authorization_endpoint: `${issuer}/authorize`,
          token_endpoint: `${issuer}/token`,
          jwks_uri: `${issuer}/jwks`,
          response_types_supported: ["code"],
          subject_types_supported: ["public"],
          id_token_signing_alg_values_supported: ["ES256"],
        });
      }
      if (request.method === "GET" && url.pathname === "/jwks") {
        return jsonResponse({ keys: [signingJwk] });
      }
      if (request.method === "GET" && url.pathname === "/authorize") {
        const redirectUri = url.searchParams.get("redirect_uri");
        const state = url.searchParams.get("state");
        const nonce = url.searchParams.get("nonce");
        if (!redirectUri || !state || !nonce) {
          return jsonResponse({
            error: "invalid_request",
          }, 400);
        }
        const code = crypto.randomUUID();
        authorizationCodes.set(code, nonce);
        const callback = new URL(redirectUri);
        callback.searchParams.set("code", code);
        callback.searchParams.set("state", state);
        return Response.redirect(callback, 302);
      }
      if (request.method === "POST" && url.pathname === "/token") {
        const form = new URLSearchParams(await request.text());
        const code = form.get("code");
        const nonce = code ? authorizationCodes.get(code) : undefined;
        if (!code || !nonce) {
          return jsonResponse(
            { error: "invalid_grant" },
            400,
          );
        }
        authorizationCodes.delete(code);
        const header = encodedJson({
          alg: "ES256",
          kid: "e2e-key",
          typ: "JWT",
        });
        const payload = encodedJson({
          iss: issuer,
          sub: subject,
          aud: form.get("client_id") ?? "e2e-client",
          iat: Math.floor(Date.now() / 1000),
          exp: Math.floor(Date.now() / 1000) + 600,
          nonce,
        });
        const signingInput = `${header}.${payload}`;
        const signature = new Uint8Array(
          await crypto.subtle.sign(
            { name: "ECDSA", hash: "SHA-256" },
            keyPair.privateKey,
            encoder.encode(signingInput),
          ),
        );
        return jsonResponse({
          access_token: "e2e-access-token",
          token_type: "Bearer",
          id_token: `${signingInput}.${base64Url(signature)}`,
        });
      }
      return new Response("Not found", { status: 404 });
    },
  );
  const address = server.addr as Deno.NetAddr;
  issuer = `http://${advertisedHost}:${address.port}`;

  return { issuer, close: () => server.shutdown() };
}
