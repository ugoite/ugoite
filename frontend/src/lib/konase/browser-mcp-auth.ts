export type BrowserMcpTarget = {
  resource: string;
  endpoint: string;
  deviceAuthorizationEndpoint: string;
  tokenEndpoint: string;
};

export type BrowserMcpCredential = {
  accessToken: string;
  endpoint: string;
  resource: string;
  spaceUid: string;
};

export type BrowserMcpApproval = {
  verificationUriComplete: string;
  userCode: string;
};

export type BrowserMcpAuthorizationOptions = {
  spaceUid: string;
  deviceName?: string;
  requestedActions?: string[];
  fetcher?: typeof fetch;
  sleep?: (milliseconds: number) => Promise<void>;
  now?: () => number;
  onApprovalRequired?: (approval: BrowserMcpApproval) => void;
};

type JsonRecord = Record<string, unknown>;

const protectedResourcePath = "/.well-known/oauth-protected-resource";
const authorizationServerPath = "/.well-known/oauth-authorization-server";
const authorizationCandidates = ["/api", ""];
const defaultActions = ["read", "create", "update"];

/** Discover the browser-visible MCP route and its exact OAuth resource. */
export const discoverBrowserMcpTarget = async (
  fetcher: typeof fetch = fetch,
): Promise<BrowserMcpTarget> => {
  const origin = browserOrigin();
  const failures: string[] = [];

  for (const base of authorizationCandidates) {
    const metadataUrl = `${base}${protectedResourcePath}`;
    const metadataResponse = await fetchJson(fetcher, metadataUrl, {
      method: "GET",
      headers: { accept: "application/json" },
    });
    if (!metadataResponse.response.ok) {
      failures.push(`${metadataUrl}: HTTP ${metadataResponse.response.status}`);
      continue;
    }

    try {
      const resource = sameOriginUrl(
        requiredString(metadataResponse.payload.resource, "MCP resource"),
        origin,
        "MCP resource",
      );
      if (resource.pathname !== "/mcp" || resource.search || resource.hash) {
        throw new Error("MCP resource must be the canonical /mcp path");
      }
      const authorizationServer = sameOriginUrl(
        firstString(
          metadataResponse.payload.authorization_servers,
          "authorization server",
        ),
        origin,
        "MCP authorization server",
      );
      const authorizationMetadataUrls = [
        `${base}${authorizationServerPath}`,
        new URL(authorizationServerPath, authorizationServer).toString(),
      ].filter((url, index, values) => values.indexOf(url) === index);
      let authorizationMetadata: {
        response: Response;
        payload: JsonRecord;
      } | undefined;
      const metadataFailures: string[] = [];
      for (const authorizationMetadataUrl of authorizationMetadataUrls) {
        const candidate = await fetchJson(fetcher, authorizationMetadataUrl, {
          method: "GET",
          headers: { accept: "application/json" },
        });
        if (!candidate.response.ok) {
          metadataFailures.push(
            `${authorizationMetadataUrl}: HTTP ${candidate.response.status}`,
          );
          continue;
        }
        try {
          requiredString(
            candidate.payload.device_authorization_endpoint,
            "device authorization endpoint",
          );
          requiredString(
            candidate.payload.token_endpoint,
            "token endpoint",
          );
          authorizationMetadata = candidate;
          break;
        } catch (error) {
          metadataFailures.push(
            `${authorizationMetadataUrl}: ${
              error instanceof Error ? error.message : String(error)
            }`,
          );
        }
      }
      if (!authorizationMetadata) {
        throw new Error(
          `OAuth metadata discovery failed: ${metadataFailures.join("; ")}`,
        );
      }
      const deviceAuthorizationEndpoint = sameOriginUrl(
        requiredString(
          authorizationMetadata.payload.device_authorization_endpoint,
          "device authorization endpoint",
        ),
        origin,
        "device authorization endpoint",
      ).toString();
      const tokenEndpoint = sameOriginUrl(
        requiredString(
          authorizationMetadata.payload.token_endpoint,
          "token endpoint",
        ),
        origin,
        "token endpoint",
      ).toString();
      return {
        resource: resource.toString(),
        endpoint: base ? `${base}/mcp` : "/mcp",
        deviceAuthorizationEndpoint,
        tokenEndpoint,
      };
    } catch (error) {
      failures.push(
        `${metadataUrl}: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }

  throw new Error(
    `Ugoite MCP discovery failed${
      failures.length ? `: ${failures.join("; ")}` : ""
    }`,
  );
};

/** Obtain an opaque MCP credential bound to one current Space. */
export const authorizeBrowserMcp = async (
  options: BrowserMcpAuthorizationOptions,
): Promise<BrowserMcpCredential> => {
  if (!options.spaceUid.trim()) {
    throw new Error("Current Space UID is required for MCP access");
  }
  const fetcher = options.fetcher ?? fetch;
  const target = await discoverBrowserMcpTarget(fetcher);
  const keyPair = await generateSigningKey();
  const publicKeyJwk = await exportPublicKey(keyPair.publicKey);
  const device = await postJson(fetcher, target.deviceAuthorizationEndpoint, {
    device_name: options.deviceName ?? "Ugoite Browser Konase",
    public_key_jwk: publicKeyJwk,
    space_uid: options.spaceUid,
    requested_actions: options.requestedActions ?? defaultActions,
    resource: target.resource,
  });
  if (!device.response.ok) {
    throw new Error(
      `MCP device authorization failed (${device.response.status})`,
    );
  }

  const deviceCode = requiredString(device.payload.device_code, "device code");
  const userCode = requiredString(device.payload.user_code, "user code");
  const verificationUri = sameOriginUrl(
    requiredString(
      device.payload.verification_uri_complete ??
        device.payload.verification_uri,
      "verification URI",
    ),
    browserOrigin(),
    "verification URI",
  );
  if (!device.payload.verification_uri_complete) {
    verificationUri.searchParams.set("user_code", userCode);
  }
  options.onApprovalRequired?.({
    verificationUriComplete: verificationUri.toString(),
    userCode,
  });

  const expiresIn = positiveNumber(device.payload.expires_in, 600);
  let interval = positiveNumber(device.payload.interval, 5);
  const now = options.now ?? Date.now;
  const expiresAt = now() + expiresIn * 1000;
  const sleep = options.sleep ??
    ((milliseconds) =>
      new Promise((resolve) => setTimeout(resolve, milliseconds)));
  while (now() < expiresAt) {
    const assertion = await clientAssertion(
      keyPair.privateKey,
      publicKeyJwk,
      target.tokenEndpoint,
    );
    const token = await postJson(fetcher, target.tokenEndpoint, {
      grant_type: "urn:ietf:params:oauth:grant-type:device_code",
      device_code: deviceCode,
      client_assertion: assertion,
      resource: target.resource,
    });
    if (token.response.ok) {
      const accessToken = requiredString(
        token.payload.access_token,
        "access token",
      );
      const tokenSpaceUid = requiredString(
        token.payload.space_uid,
        "token Space UID",
      );
      if (tokenSpaceUid !== options.spaceUid) {
        throw new Error("MCP credential is bound to a different Space");
      }
      return {
        accessToken,
        endpoint: target.endpoint,
        resource: target.resource,
        spaceUid: tokenSpaceUid,
      };
    }
    const error = typeof token.payload.error === "string"
      ? token.payload.error
      : undefined;
    if (error !== "authorization_pending" && error !== "slow_down") {
      throw new Error(
        `MCP device authorization failed (${token.response.status})`,
      );
    }
    if (error === "slow_down") interval += 5;
    await sleep(interval * 1000);
  }
  throw new Error("MCP device authorization expired");
};

const generateSigningKey = async (): Promise<CryptoKeyPair> =>
  await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["sign", "verify"],
  ) as CryptoKeyPair;

const exportPublicKey = async (key: CryptoKey): Promise<JsonRecord> => {
  const jwk = await crypto.subtle.exportKey("jwk", key) as JsonRecord;
  if (
    jwk.kty !== "EC" || jwk.crv !== "P-256" || typeof jwk.x !== "string" ||
    typeof jwk.y !== "string"
  ) {
    throw new Error("browser generated an invalid MCP signing key");
  }
  return { kty: jwk.kty, crv: jwk.crv, x: jwk.x, y: jwk.y };
};

const clientAssertion = async (
  key: CryptoKey,
  jwk: JsonRecord,
  audience: string,
): Promise<string> => {
  const now = Math.floor(Date.now() / 1000);
  const x = String(jwk.x);
  const y = String(jwk.y);
  const clientId = encodeBase64Url(
    await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(
        `{"crv":"P-256","kty":"EC","x":"${x}","y":"${y}"}`,
      ),
    ),
  );
  const header = encodeBase64Url(
    new TextEncoder().encode(JSON.stringify({ alg: "ES256", typ: "JWT", jwk })),
  );
  const payload = encodeBase64Url(
    new TextEncoder().encode(JSON.stringify({
      iss: clientId,
      sub: clientId,
      aud: audience,
      iat: now,
      exp: now + 60,
      jti: crypto.randomUUID(),
    })),
  );
  const input = `${header}.${payload}`;
  const signature = await crypto.subtle.sign(
    { name: "ECDSA", hash: "SHA-256" },
    key,
    new TextEncoder().encode(input),
  );
  return `${input}.${encodeBase64Url(signature)}`;
};

const fetchJson = async (
  fetcher: typeof fetch,
  input: RequestInfo | URL,
  init: RequestInit,
): Promise<{ response: Response; payload: JsonRecord }> => {
  const response = await fetcher(input, {
    ...init,
    credentials: "same-origin",
  });
  return { response, payload: await readJson(response) };
};

const postJson = async (
  fetcher: typeof fetch,
  input: RequestInfo | URL,
  value: JsonRecord,
) =>
  await fetchJson(fetcher, input, {
    method: "POST",
    headers: { accept: "application/json", "content-type": "application/json" },
    body: JSON.stringify(value),
  });

const readJson = async (response: Response): Promise<JsonRecord> => {
  const value: unknown = await response.json().catch(() => ({}));
  return isRecord(value) ? value : {};
};

const requiredString = (value: unknown, label: string): string => {
  if (typeof value !== "string" || !value.trim()) {
    throw new Error(`${label} is missing`);
  }
  return value;
};

const firstString = (value: unknown, label: string): string => {
  if (!Array.isArray(value)) throw new Error(`${label} is missing`);
  return requiredString(value[0], label);
};

const positiveNumber = (value: unknown, fallback: number): number =>
  typeof value === "number" && Number.isFinite(value) && value > 0
    ? value
    : fallback;

const browserOrigin = (): string => {
  const origin = globalThis.location?.origin;
  if (!origin) throw new Error("browser origin is unavailable");
  return origin;
};

const sameOriginUrl = (value: string, origin: string, label: string): URL => {
  const url = new URL(value, origin);
  if (url.origin !== origin || url.username || url.password) {
    throw new Error(`${label} must use the current browser origin`);
  }
  return url;
};

const encodeBase64Url = (value: ArrayBuffer | Uint8Array): string => {
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(
    /=+$/,
    "",
  );
};

const isRecord = (value: unknown): value is JsonRecord =>
  typeof value === "object" && value !== null && !Array.isArray(value);
