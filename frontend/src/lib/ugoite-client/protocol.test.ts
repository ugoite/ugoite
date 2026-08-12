import { describe, expect, it } from "vitest";
import {
  getWasmSupportedOperations,
  prepareApiRequest,
  validateAssetReference,
  UGOITE_API_OPERATIONS,
  UGOITE_WASM_PROTOCOL_VERSION,
} from "./protocol";

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
});
