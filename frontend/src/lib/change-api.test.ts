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
        target_form_ids: ["form-1"],
        field_groups: [{
          form_id: "form-1",
          field_id: 100,
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
        sort: [
          { field: "actor_principal_id", direction: "asc" },
          { field: "created_at_micros", direction: "desc" },
        ],
      }),
    ).resolves.toEqual({
      changes: [{
        ...row,
        summary: {
          affected_entry_count: 2,
          target_form_ids: ["form-1"],
          field_groups: [{
            form_id: "form-1",
            field_id: 100,
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
    expect(received?.searchParams.get("sort")).toBe(
      "actor_principal_id:asc,created_at_micros:desc",
    );
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

describe("changeApi inspection", () => {
  it("loads a bounded Change detail page with its opaque cursor", async () => {
    let received: URL | undefined;
    server.use(
      http.get(testApiUrl("/spaces/space-1/changes/change-1/inspect"), ({ request }) => {
        received = new URL(request.url);
        return HttpResponse.json({
          change_id: "change-1",
          change: { actor_principal_id: "principal-1", created_at_micros: 100 },
          target_visibility: "complete",
          summary: null,
          targets: [{
            form_id: "form-1", entry_id: "entry-1", before_revision_id: "revision-0",
            after_revision_id: "revision-1", operation: "update", fields: [],
          }],
          next_cursor: "opaque-next",
        });
      }),
    );

    const page = await changeApi.inspect("space-1", "change-1", { limit: 8, cursor: "opaque cursor" });
    expect(page.targets[0]).toMatchObject({ form_id: "form-1", entry_id: "entry-1", operation: "update" });
    expect(page.next_cursor).toBe("opaque-next");
    expect(received?.searchParams.get("limit")).toBe("8");
    expect(received?.searchParams.get("cursor")).toBe("opaque cursor");
  });

  it("loads selected Entry field evidence through the authorized target endpoint", async () => {
    let received: URL | undefined;
    server.use(
      http.get(testApiUrl("/spaces/space-1/changes/change-1/affected/entry-1"), ({ request }) => {
        received = new URL(request.url);
        return HttpResponse.json({
          change_id: "change-1",
          target: {
            form_id: "form-1", entry_id: "entry-1", before_revision_id: "revision-0",
            after_revision_id: "revision-1", operation: "update",
            fields: [{ field_id: 1, before: { state: "value", value: "Draft" }, after: { state: "value", value: "Approved" } }],
          },
        });
      }),
    );

    const entry = await changeApi.affectedEntry("space-1", "change-1", "entry-1");
    expect(entry.fields[0]).toEqual({ field_id: 1, before: { state: "value", value: "Draft" }, after: { state: "value", value: "Approved" } });
    expect(received?.pathname).toBe("/api/spaces/space-1/changes/change-1/affected/entry-1");
  });
});
