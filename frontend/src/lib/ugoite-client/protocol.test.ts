import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import {
  getWasmSupportedOperations,
  prepareApiRequest,
  protocolFetch,
  protocolFetchResponse,
  UGOITE_API_OPERATIONS,
  UGOITE_WASM_PROTOCOL_VERSION,
  UgoiteApiError,
  validateAssetReference,
} from "./protocol";
import { testApiUrl } from "~/test/http-origin";
import { server } from "~/test/mocks/server";

const validReference = {
  asset_id: "01900000-0000-7000-8000-000000000001",
  name: "report.pdf",
  media_type: "application/pdf",
  size_bytes: 10,
  sha256: "a".repeat(64),
};

describe("portable Ugoite API protocol WASM", () => {
  it("uses the Rust domain contract for AssetReference validation", async () => {
    await expect(
      validateAssetReference({ ...validReference, asset_id: "../asset" }),
    ).rejects.toThrow();
    await expect(validateAssetReference(validReference)).resolves.toEqual(
      validReference,
    );
  });

  it("REQ-API-001: exposes the expected ABI version and operation manifest", async () => {
    expect(UGOITE_WASM_PROTOCOL_VERSION).toBe(1);
    await expect(getWasmSupportedOperations()).resolves.toEqual(
      UGOITE_API_OPERATIONS,
    );
  });

  it("REQ-API-001: encodes path segments and query values in Rust/WASM", async () => {
    const request = await prepareApiRequest("search.keyword", {
      space_id: "team/東京",
      q: "a & b#c",
    });

    expect(request).toMatchObject({
      method: "GET",
      path: "/spaces/team%2F%E6%9D%B1%E4%BA%AC/search?q=a+%26+b%23c",
      body_kind: "none",
    });
  });

  it("REQ-API-001: serializes JSON bodies in the portable Rust layer", async () => {
    const request = await prepareApiRequest(
      "entry.create",
      { space_id: "demo" },
      { id: "entry-1", markdown: "# Hello" },
    );

    expect(request.method).toBe("POST");
    expect(request.body_kind).toBe("json");
    expect(request.headers).toContainEqual({
      name: "content-type",
      value: "application/json",
    });
    expect(JSON.parse(request.body ?? "null")).toEqual({
      id: "entry-1",
      markdown: "# Hello",
    });
  });

  it("test_req_sec_012_owner_recovery_protocol_forwards_header", async () => {
    const forceReset = await prepareApiRequest(
      "space.recovery.force_reset",
      { space_id: "team" },
      { principal_id: "01900000-0000-7000-8000-000000000001" },
    );
    expect(forceReset).toMatchObject({
      method: "POST",
      path: "/spaces/team/admin/recovery/force-reset",
      body_kind: "json",
    });
    expect(forceReset.headers).toContainEqual({
      name: "content-type",
      value: "application/json",
    });
    expect(forceReset.headers).not.toContainEqual({
      name: "idempotency-key",
      value: expect.any(String),
    });

    const request = await prepareApiRequest(
      "space.recovery.backup_codes",
      {
        space_id: "team",
        idempotency_key: "018f1f3a-9d7b-4e1b-8e3a-6e8a4a6d1f12",
      },
      { principal_id: "01900000-0000-7000-8000-000000000001" },
    );

    expect(request).toMatchObject({
      method: "POST",
      path: "/spaces/team/admin/recovery/backup-codes",
    });
    expect(request.headers).toContainEqual({
      name: "idempotency-key",
      value: "018f1f3a-9d7b-4e1b-8e3a-6e8a4a6d1f12",
    });

    const ownerStart = await prepareApiRequest(
      "auth.recovery.owner_start",
      {},
      { owner_approval_token: "owner-token" },
    );
    expect(ownerStart).toMatchObject({
      method: "POST",
      path: "/auth/recovery/owner/start",
    });

    const ownerFinish = await prepareApiRequest(
      "auth.recovery.owner_finish",
      {},
      {
        challenge_id: "01900000-0000-7000-8000-000000000002",
        credential: { id: "credential" },
      },
    );
    expect(ownerFinish).toMatchObject({
      method: "POST",
      path: "/auth/recovery/owner/finish",
    });
    expect(UGOITE_API_OPERATIONS).toEqual(
      expect.arrayContaining([
        "space.recovery.force_reset",
        "space.recovery.backup_codes",
        "auth.recovery.owner_start",
        "auth.recovery.owner_finish",
      ]),
    );
  });

  it("REQ-API-001: executes JSON protocol operations through the browser adapter", async () => {
    server.use(
      http.get(testApiUrl("/spaces/demo"), ({ request }) => {
        expect(request.headers.get("x-test-header")).toBe("kept");
        return HttpResponse.json({ id: "demo", name: "Demo" });
      }),
      http.post(testApiUrl("/spaces/demo/entries"), async ({ request }) => {
        expect(request.headers.get("content-type")).toBe("application/json");
        expect(await request.json()).toEqual({ id: "entry-1" });
        return HttpResponse.json({ id: "entry-1", revision_id: "rev-1" });
      }),
    );

    await expect(
      protocolFetch(
        "space.get",
        { space_id: "demo" },
        undefined,
        { headers: { "x-test-header": "kept" }, trackLoading: false },
      ),
    ).resolves.toEqual({ id: "demo", name: "Demo" });

    await expect(
      protocolFetch(
        "entry.create",
        { space_id: "demo" },
        { id: "entry-1" },
        { trackLoading: false },
      ),
    ).resolves.toEqual({ id: "entry-1", revision_id: "rev-1" });
  });

  it("REQ-API-001: preserves multipart bodies and returns binary responses", async () => {
    const form = new FormData();
    form.append("file", new Blob(["contents"]), "note.txt");
    server.use(
      http.post(testApiUrl("/spaces/demo/assets"), ({ request }) => {
        expect(request.headers.get("content-type")).toContain(
          "multipart/form-data",
        );
        return HttpResponse.json({ status: "uploaded" });
      }),
      http.get(
        testApiUrl("/spaces/demo/assets/asset-1?form=Note&entry_id=entry-1"),
        () => new HttpResponse("asset bytes", { status: 200 }),
      ),
    );

    await expect(
      protocolFetch(
        "asset.upload",
        { space_id: "demo" },
        undefined,
        { body: form, trackLoading: false },
      ),
    ).resolves.toEqual({ status: "uploaded" });

    const response = await protocolFetchResponse("asset.read", {
      space_id: "demo",
      asset_id: "asset-1",
      form: "Note",
      entry_id: "entry-1",
    }, { trackLoading: false });
    await expect(response.text()).resolves.toBe("asset bytes");
  });

  it("REQ-API-001: exposes protocol errors with stable error-code fallbacks", () => {
    expect(
      new UgoiteApiError({
        kind: "invalid_arguments",
        message: "invalid",
        detail: { code: "from-detail" },
      }).code,
    ).toBe("from-detail");
    expect(
      new UgoiteApiError({
        kind: "invalid_arguments",
        message: "invalid",
        detail: { code: 42 },
        payload: { code: "from-payload" },
      }).code,
    ).toBe("from-payload");
    expect(
      new UgoiteApiError({
        kind: "invalid_arguments",
        message: "invalid",
        detail: null,
        payload: "not-an-object",
      }).code,
    ).toBeUndefined();
  });

  it("REQ-API-001: turns non-success protocol responses into UgoiteApiError", async () => {
    server.use(
      http.get(testApiUrl("/spaces/missing"), () =>
        HttpResponse.json({
          kind: "not_found",
          message: "space not found",
          code: "space_missing",
        }, { status: 404 })),
    );

    await expect(
      protocolFetch("space.get", { space_id: "missing" }, undefined, {
        trackLoading: false,
      }),
    ).rejects.toMatchObject({
      name: "UgoiteApiError",
      code: "space_missing",
      status: 404,
    });

    server.use(
      http.get(
        testApiUrl("/spaces/missing/assets/asset-1"),
        () =>
          HttpResponse.json({
            kind: "not_found",
            message: "asset not found",
          }, { status: 404 }),
      ),
    );
    await expect(
      protocolFetchResponse("asset.read", {
        space_id: "missing",
        asset_id: "asset-1",
        form: "Note",
        entry_id: "entry-1",
      }, { trackLoading: false }),
    ).rejects.toMatchObject({ name: "UgoiteApiError", status: 404 });
  });
});
