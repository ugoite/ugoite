import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";
import { authApi } from "./auth-api";

describe("invitation registration finalization", () => {
  it("resumes a durable Passkey claim without creating another credential", async () => {
    let finishBody: Record<string, unknown> | undefined;
    server.use(
      http.post(testApiUrl("/auth/invitations/start"), () =>
        HttpResponse.json({ status: "resume" })),
      http.post(testApiUrl("/auth/invitations/finish"), async ({ request }) => {
        finishBody = await request.json() as Record<string, unknown>;
        return HttpResponse.json({ account: {} });
      }),
    );

    await authApi.registerInvitation("invitation-token");

    expect(finishBody).toEqual({
      invitation_token: "invitation-token",
      resume: true,
    });
  });
});
