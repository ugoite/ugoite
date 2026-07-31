import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";
import { authApi } from "./auth-api";

describe("invitation registration finalization", () => {
  it("re-authenticates with Passkey before accepting a durable claim", async () => {
    server.use(
      http.post(testApiUrl("/auth/invitations/start"), () =>
        HttpResponse.json({ status: "resume" })),
    );

    const loginWithPasskey = vi.spyOn(authApi, "loginWithPasskey")
      .mockResolvedValue();
    const acceptInvitation = vi.spyOn(authApi, "acceptInvitation")
      .mockResolvedValue();
    try {
      await authApi.registerInvitation("invitation-token");
      expect(loginWithPasskey).toHaveBeenCalledOnce();
      expect(acceptInvitation).toHaveBeenCalledWith("invitation-token");
    } finally {
      loginWithPasskey.mockRestore();
      acceptInvitation.mockRestore();
    }
  });
});
