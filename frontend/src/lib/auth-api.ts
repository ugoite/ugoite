export type AuthConfig = {
  status: "uninitialized" | "active";
  nodeId: string;
  issuer: string;
  rpId: string;
  passkey: boolean;
  oidc: boolean;
};

export type AuthSessionState = {
  authenticated: boolean;
  account?: { account_id: string; display_name: string };
};

export type OidcProvider = {
  provider_id: string;
  issuer: string;
  client_id: string;
};

type ChallengeEnvelope = {
  challenge_id: string;
  public_key: {
    publicKey:
      | PublicKeyCredentialCreationOptions
      | PublicKeyCredentialRequestOptions;
  };
};

const request = async <T>(path: string, init?: RequestInit): Promise<T> => {
  const response = await fetch(`/api${path}`, {
    ...init,
    credentials: "same-origin",
    headers: { "content-type": "application/json", ...init?.headers },
  });
  const payload = await response.json().catch(() => ({})) as Record<
    string,
    unknown
  >;
  if (!response.ok) {
    const message = typeof payload.message === "string"
      ? payload.message
      : typeof payload.detail === "string"
      ? payload.detail
      : `Authentication failed (${response.status})`;
    throw new Error(message);
  }
  return payload as T;
};

const decode = (value: string): ArrayBuffer => {
  const padded = value.replace(/-/g, "+").replace(/_/g, "/").padEnd(
    Math.ceil(value.length / 4) * 4,
    "=",
  );
  return Uint8Array.from(atob(padded), (character) => character.charCodeAt(0))
    .buffer;
};

const encode = (value: ArrayBuffer): string => {
  const binary = String.fromCharCode(...new Uint8Array(value));
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(
    /=+$/,
    "",
  );
};

const creationOptions = (
  options: PublicKeyCredentialCreationOptions,
): PublicKeyCredentialCreationOptions => ({
  ...options,
  challenge: decode(options.challenge as unknown as string),
  user: { ...options.user, id: decode(options.user.id as unknown as string) },
  excludeCredentials: options.excludeCredentials?.map((credential) => ({
    ...credential,
    id: decode(credential.id as unknown as string),
  })),
});

const requestOptions = (
  options: PublicKeyCredentialRequestOptions,
): PublicKeyCredentialRequestOptions => ({
  ...options,
  challenge: decode(options.challenge as unknown as string),
  allowCredentials: options.allowCredentials?.map((credential) => ({
    ...credential,
    id: decode(credential.id as unknown as string),
  })),
});

const serializeCredential = (
  credential: PublicKeyCredential,
): Record<string, unknown> => {
  const response = credential.response;
  if (response instanceof AuthenticatorAttestationResponse) {
    return {
      id: credential.id,
      rawId: encode(credential.rawId),
      type: credential.type,
      response: {
        attestationObject: encode(response.attestationObject),
        clientDataJSON: encode(response.clientDataJSON),
        transports: response.getTransports?.() ?? [],
      },
      clientExtensionResults: credential.getClientExtensionResults(),
    };
  }
  const assertion = response as AuthenticatorAssertionResponse;
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    response: {
      authenticatorData: encode(assertion.authenticatorData),
      clientDataJSON: encode(assertion.clientDataJSON),
      signature: encode(assertion.signature),
      userHandle: assertion.userHandle ? encode(assertion.userHandle) : null,
    },
    clientExtensionResults: credential.getClientExtensionResults(),
  };
};

const createPasskey = async (
  challenge: ChallengeEnvelope,
): Promise<Record<string, unknown>> => {
  const credential = await navigator.credentials.create({
    publicKey: creationOptions(
      challenge.public_key.publicKey as PublicKeyCredentialCreationOptions,
    ),
  });
  if (!(credential instanceof PublicKeyCredential)) {
    throw new Error("Passkey registration was cancelled.");
  }
  return serializeCredential(credential);
};

export const authApi = {
  async getSession(): Promise<AuthSessionState> {
    return await request<AuthSessionState>("/auth/session", { method: "GET" });
  },
  async getConfig(): Promise<AuthConfig> {
    const payload = await request<Record<string, unknown>>("/auth/config", {
      method: "GET",
    });
    return {
      status: payload.status as AuthConfig["status"],
      nodeId: String(payload.node_id),
      issuer: String(payload.issuer),
      rpId: String(payload.rp_id),
      passkey: payload.passkey === true,
      oidc: payload.oidc === true,
    };
  },
  async listOidcProviders(): Promise<OidcProvider[]> {
    return await request("/auth/oidc/providers", { method: "GET" });
  },
  loginWithOidc(providerId: string, invitationToken?: string): void {
    const query = invitationToken
      ? `?invitation_token=${encodeURIComponent(invitationToken)}`
      : "";
    location.assign(`/api/auth/oidc/${encodeURIComponent(providerId)}/start${query}`);
  },
  linkOidc(providerId: string): void {
    location.assign(`/api/auth/oidc/${encodeURIComponent(providerId)}/link`);
  },
  async loginWithPasskey(): Promise<void> {
    const challenge = await request<ChallengeEnvelope>("/auth/passkey/start", {
      method: "POST",
      body: "{}",
    });
    const credential = await navigator.credentials.get({
      publicKey: requestOptions(
        challenge.public_key.publicKey as PublicKeyCredentialRequestOptions,
      ),
      mediation: "optional",
    });
    if (!(credential instanceof PublicKeyCredential)) {
      throw new Error("Passkey login was cancelled.");
    }
    await request("/auth/passkey/finish", {
      method: "POST",
      body: JSON.stringify({
        challenge_id: challenge.challenge_id,
        credential: serializeCredential(credential),
      }),
    });
  },
  async setup(
    setupSecret: string,
    displayName: string,
  ): Promise<{
    recovery_codes: string[];
    account: { account_id: string; display_name: string };
  }> {
    const challenge = await request<ChallengeEnvelope>("/auth/setup/start", {
      method: "POST",
      body: JSON.stringify({
        setup_secret: setupSecret,
        display_name: displayName,
      }),
    });
    const credential = await createPasskey(challenge);
    return await request("/auth/setup/finish", {
      method: "POST",
      body: JSON.stringify({
        setup_secret: setupSecret,
        challenge_id: challenge.challenge_id,
        credential,
      }),
    });
  },
  async registerInvitation(invitationToken: string): Promise<void> {
    const challenge = await request<ChallengeEnvelope>(
      "/auth/invitations/start",
      {
        method: "POST",
        body: JSON.stringify({ invitation_token: invitationToken }),
      },
    );
    const credential = await createPasskey(challenge);
    await request("/auth/invitations/finish", {
      method: "POST",
      body: JSON.stringify({
        invitation_token: invitationToken,
        challenge_id: challenge.challenge_id,
        credential,
      }),
    });
  },
  async acceptInvitation(invitationToken: string): Promise<void> {
    await protocolFetch(
      "auth.accept_invitation",
      {},
      { invitation_token: invitationToken },
    );
  },
  async listPasskeys(): Promise<Array<Record<string, unknown>>> {
    return await request("/auth/passkeys", { method: "GET" });
  },
  async addPasskey(): Promise<void> {
    const challenge = await request<ChallengeEnvelope>("/auth/passkeys/start", {
      method: "POST",
      body: "{}",
    });
    const credential = await createPasskey(challenge);
    await request("/auth/passkeys/finish", {
      method: "POST",
      body: JSON.stringify({
        challenge_id: challenge.challenge_id,
        credential,
      }),
    });
  },
  async revokePasskey(credentialId: string): Promise<void> {
    await request(`/auth/passkeys/${encodeURIComponent(credentialId)}`, {
      method: "DELETE",
    });
  },
  async listSessions(): Promise<Array<Record<string, unknown>>> {
    return await protocolFetch("auth.list_sessions");
  },
  async revokeSession(sessionId: string): Promise<void> {
    await protocolFetch("auth.revoke_session", { session_id: sessionId });
  },
  async listDevices(): Promise<Array<Record<string, unknown>>> {
    return await request("/auth/devices", { method: "GET" });
  },
  async revokeDevice(credentialId: string): Promise<void> {
    await request(`/auth/devices/${encodeURIComponent(credentialId)}`, {
      method: "DELETE",
    });
  },
  async startTotpEnrollment(): Promise<{
    secret: string;
    otpauth_uri: string;
  }> {
    return await request("/auth/recovery/totp/start", {
      method: "POST",
      body: "{}",
    });
  },
  async finishTotpEnrollment(code: string): Promise<void> {
    await request("/auth/recovery/totp/finish", {
      method: "POST",
      body: JSON.stringify({ code }),
    });
  },
  async recoverPasskey(
    accountId: string,
    recoveryCode: string,
    totpCode: string,
  ): Promise<{ recovery_codes: string[] }> {
    const challenge = await request<ChallengeEnvelope>("/auth/recovery/start", {
      method: "POST",
      body: JSON.stringify({
        account_id: accountId,
        recovery_code: recoveryCode,
        totp_code: totpCode,
      }),
    });
    const credential = await createPasskey(challenge);
    return await request<{ recovery_codes: string[] }>("/auth/recovery/finish", {
      method: "POST",
      body: JSON.stringify({
        account_id: accountId,
        challenge_id: challenge.challenge_id,
        credential,
      }),
    });
  },
  async clearSession(): Promise<void> {
    await request("/auth/session", { method: "DELETE" });
  },
};
import { protocolFetch } from "./ugoite-client/protocol";
