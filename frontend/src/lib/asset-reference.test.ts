import { describe, expect, it } from "vitest";
import {
  formatAssetSize,
  hasDuplicateAssetReferences,
  isAssetReference,
  parseAssetReference,
  parseAssetReferenceList,
  serializeAssetReference,
  serializeAssetReferenceList,
} from "./asset-reference";
import type { AssetReference } from "./types";

const reference: AssetReference = {
  asset_id: "01900000-0000-7000-8000-000000000001",
  name: "report.pdf",
  media_type: "application/pdf",
  size_bytes: 123456,
  sha256: "a".repeat(64),
};

describe("AssetReference value helpers", () => {
  it("preserves the complete reference through JSON Markdown values", () => {
    const encoded = serializeAssetReference(reference);

    expect(parseAssetReference(encoded)).toEqual(reference);
    expect(encoded).toContain('"asset_id"');
    expect(encoded).toContain('"sha256"');
  });

  it("preserves ordered typed lists and rejects malformed values", () => {
    const second = {
      ...reference,
      asset_id: "01900000-0000-7000-8000-000000000002",
    };
    const encoded = serializeAssetReferenceList([reference, second]);

    expect(parseAssetReferenceList(encoded)).toEqual([reference, second]);
    expect(parseAssetReference('{"name":"report.pdf"}')).toBeNull();
    expect(parseAssetReferenceList('["report.pdf"]')).toBeNull();
    expect(isAssetReference({ ...reference, sha256: "not-a-checksum" })).toBe(
      false,
    );
  });

  it("detects duplicate references and formats sizes for people", () => {
    expect(hasDuplicateAssetReferences([reference, reference])).toBe(true);
    expect(
      hasDuplicateAssetReferences([
        reference,
        { ...reference, asset_id: "01900000-0000-7000-8000-000000000002" },
      ]),
    ).toBe(false);
    expect(formatAssetSize(123456)).toMatch(/bytes|KB|KiB/);
  });
});
