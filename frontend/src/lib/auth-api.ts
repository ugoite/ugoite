import { protocolFetch } from "./ugoite-client/protocol";

export type AuthConfig = {
  mode: "passkey-totp" | "mock-oauth";
  usernameHint: string;
  supportsPasskeyTotp: boolean;
  supportsMockOauth: boolean;
};

export type AuthLoginResponse = {
  userId: string;
  expiresAt: number;
};

export type AuthSessionState = {
  authenticated: boolean;
};

const readString = (payload: Record<string, unknown>, key: string): string => {
  const value = payload[key];
  if (typeof value !== "string") {
    throw new Error(`Invalid auth response: ${key} must be a string.`);
  }
  return value;
};

const readBoolean = (
  payload: Record<string, unknown>,
  key: string,
): boolean => {
  const value = payload[key];
  if (typeof value !== "boolean") {
    throw new Error(`Invalid auth response: ${key} must be a boolean.`);
  }
  return value;
};

const readNumber = (payload: Record<string, unknown>, key: string): number => {
  const value = payload[key];
  if (typeof value !== "number") {
    throw new Error(`Invalid auth response: ${key} must be a number.`);
  }
  return value;
};

export const authApi = {
  async getSession(): Promise<AuthSessionState> {
    const payload = await protocolFetch<Record<string, unknown>>(
      "auth.get_session",
      {},
      undefined,
      { trackLoading: false },
    );
    return { authenticated: readBoolean(payload, "authenticated") };
  },

  async getConfig(): Promise<AuthConfig> {
    const payload = await protocolFetch<Record<string, unknown>>(
      "auth.get_config",
      {},
      undefined,
      { trackLoading: false },
    );
    return {
      mode: readString(payload, "mode") as AuthConfig["mode"],
      usernameHint: readString(payload, "username_hint"),
      supportsPasskeyTotp: readBoolean(payload, "supports_passkey_totp"),
      supportsMockOauth: readBoolean(payload, "supports_mock_oauth"),
    };
  },

  async loginWithPasskeyTotp(
    username: string,
    totpCode: string,
  ): Promise<AuthLoginResponse> {
    const payload = await protocolFetch<Record<string, unknown>>(
      "auth.login",
      {},
      { username, totp_code: totpCode },
    );
    return {
      userId: readString(payload, "user_id"),
      expiresAt: readNumber(payload, "expires_at"),
    };
  },

  async loginWithMockOauth(): Promise<AuthLoginResponse> {
    const payload = await protocolFetch<Record<string, unknown>>(
      "auth.mock_oauth",
      {},
      {},
    );
    return {
      userId: readString(payload, "user_id"),
      expiresAt: readNumber(payload, "expires_at"),
    };
  },

  async clearSession(): Promise<void> {
    await protocolFetch<unknown>("auth.clear_session", {}, undefined, {
      trackLoading: false,
    });
  },
};
