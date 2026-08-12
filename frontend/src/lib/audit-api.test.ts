import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";
import { auditApi } from "./audit-api";

describe("auditApi", () => {
  it("loads the node audit response from the existing endpoint", async () => {
    const event = {
      event_id: "event-1",
      timestamp: "2026-08-12T08:00:00Z",
      node_id: "node-1",
      subject_account_id: "account-1",
      actor_account_id: "account-2",
      credential_id: "credential-1",
      action: "account.updated",
      target_type: "account",
      target_id: "account-1",
      outcome: "success",
      request_id: "request-1",
      safe_metadata: {},
    };
    server.use(
      http.get(testApiUrl("/auth/audit"), () => HttpResponse.json([event])),
    );

    await expect(auditApi.listNode()).resolves.toEqual([event]);
  });

  it("maps the viewer query to the Space audit contract", async () => {
    let received: URL | undefined;
    const page = { items: [], total: 0, offset: 25, limit: 25 };
    server.use(
      http.get(testApiUrl("/spaces/space-1/audit"), ({ request }) => {
        received = new URL(request.url);
        return HttpResponse.json(page);
      }),
    );

    await expect(
      auditApi.listSpace("space-1", {
        offset: 25,
        limit: 25,
        action: "authorization.denied",
        actorId: "actor-1",
        outcome: "deny",
      }),
    ).resolves.toEqual(page);

    expect(received?.search).toBe(
      "?offset=25&limit=25&action=authorization.denied&actor_principal_id=actor-1&outcome=deny",
    );
  });

  it("classifies a plain-string forbidden response for localized UI errors", async () => {
    server.use(
      http.get(
        testApiUrl("/auth/audit"),
        () => HttpResponse.text("node admin role is required", { status: 403 }),
      ),
    );

    await expect(auditApi.listNode()).rejects.toMatchObject({
      kind: "forbidden",
      status: 403,
      detail: "node admin role is required",
    });
  });
});
