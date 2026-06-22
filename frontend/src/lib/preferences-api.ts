import type { UserPreferences, UserPreferencesPatchPayload } from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

export const preferencesApi = {
  async getMe(): Promise<UserPreferences> {
    return await protocolFetch<UserPreferences>("preferences.get");
  },

  async patchMe(
    payload: UserPreferencesPatchPayload,
  ): Promise<UserPreferences> {
    return await protocolFetch<UserPreferences>("preferences.patch", {}, payload);
  },
};
