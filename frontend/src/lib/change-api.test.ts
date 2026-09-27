import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";
import { changeApi } from "./change-api";

describe("changeApi.query", () => {
  it("maps bounded filters and retains typed evidence summaries", async () => {
    let received: URL | undefined;
    const row = {
      change_id: "change-1",
      generation: 12,
      change: {
        actor_principal_id: "principal-1",
        message: "Travel update",
        reverts_change_id: null,
        run_id: "run-1",
        created_at_micros: 100,
      },
      publication: {
        generation: 12,
        publication_uri: {
          space_uid: "space-1",
          key: "_ugoite/catalog/publications/12.json",
        },
        publication_checksum: "a".repeat(64),
      },
      target_visibility: "complete",
      summary: {
        affected_entry_count: 2,
        field_groups: [{
          form_id: "form-1",
          field_id: "field-1",
          before: { state: "value", value: "Travel" },
          after: { state: "value", value: "Business travel" },
          affected_entry_count: 2,
        }],
      },
    };
    server.use(
      http.get(
        testApiUrl("/spaces/space-1/changes/query"),
        ({ request }) => {
          received = new URL(request.url);
          return HttpResponse.json({ changes: [row], next_cursor: "opaque-next" });
        },
      ),
    );

    await expect(
      changeApi.query("space-1", {
        limit: 20,
        cursor: "opaque cursor",
        actor_principal_id: "principal-1",
        text: "travel",
        created_after_micros: 50,
      }),
    ).resolves.toEqual({
      changes: [{
        ...row,
        summary: {
          affected_entry_count: 2,
          field_groups: [{
            form_id: "form-1",
            field_id: "field-1",
            before: { state: "value", value: "Travel" },
            after: { state: "value", value: "Business travel" },
            affected_entry_count: 2,
          }],
        },
      }],
      next_cursor: "opaque-next",
    });
    expect(received?.searchParams.get("limit")).toBe("20");
    expect(received?.searchParams.get("cursor")).toBe("opaque cursor");
    expect(received?.searchParams.get("actor_principal_id")).toBe("principal-1");
    expect(received?.searchParams.get("text")).toBe("travel");
    expect(received?.searchParams.get("created_after_micros")).toBe("50");
  });

  it("preserves a null summary when the current read scope is partial", async () => {
    server.use(
      http.get(
        testApiUrl("/spaces/space-1/changes/query"),
        () =>
          HttpResponse.json({
            changes: [{
              change_id: "change-1",
              generation: 12,
              change: {
                actor_principal_id: "principal-1",
                created_at_micros: 100,
              },
              publication: {
                generation: 12,
                publication_uri: {
                  space_uid: "space-1",
                  key: "_ugoite/catalog/publications/12.json",
                },
                publication_checksum: "a".repeat(64),
              },
              target_visibility: "partial",
              summary: null,
            }],
            next_cursor: null,
          }),
      ),
    );

    const page = await changeApi.query("space-1");
    expect(page.changes[0].target_visibility).toBe("partial");
    expect(page.changes[0].summary).toBeNull();
    expect(page.next_cursor).toBeNull();
  });
});
