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
});
