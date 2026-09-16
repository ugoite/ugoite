import { describe, expect, it } from "vitest";
import {
  formatAssetSummary,
  formatValueForDisplay,
  formatValueForInput,
  safeText,
} from "./display-value";

const asset = {
  asset_id: "asset-1",
  name: "hero-banner.png",
  media_type: "image/png",
  size_bytes: 1_800_000,
  sha256: "a".repeat(64),
} as const;

describe("display-value", () => {
  it("converts scalar values and rejects object coercion", () => {
    expect(safeText("text")).toBe("text");
    expect(safeText(0)).toBe("0");
    expect(safeText(false)).toBe("false");
    expect(safeText({ value: "hidden" })).toBe("");
    expect(safeText(["hidden"])).toBe("");
  });

  it("formats assets as type and size metadata", () => {
    expect(formatAssetSummary(asset)).toMatch(/^PNG · /);
    expect(formatValueForDisplay(asset)).toMatch(/^PNG · /);
  });

  it("keeps scalar lists readable and hides structured records", () => {
    expect(formatValueForDisplay(["one", "two"])).toBe("one, two");
    expect(formatValueForDisplay([{ value: "hidden" }])).toBe("-");
    expect(formatValueForInput(["one", "two"])).toBe("one, two");
    expect(formatValueForInput({ value: "hidden" })).toBe("");
  });

  it("never renders [object Object] for object-like values", () => {
    const nested = { outer: { inner: "hidden" } };
    const relationLike = { entry_id: "entry-1", title: "Alpha" };
    const arrayOfObjects = [{ value: "one" }, { value: "two" }];
    for (const value of [
      { value: "hidden" },
      nested,
      arrayOfObjects,
      relationLike,
      asset,
      [asset, { value: "hidden" }],
    ]) {
      expect(formatValueForDisplay(value)).not.toContain("[object Object]");
      expect(formatValueForInput(value)).not.toContain("[object Object]");
      expect(safeText(value)).not.toContain("[object Object]");
    }
    expect(formatValueForDisplay(nested)).toBe("-");
    expect(formatValueForDisplay(relationLike)).toBe("-");
    expect(formatValueForDisplay(arrayOfObjects)).toBe("-");
    expect(formatValueForInput(nested)).toBe("");
    expect(formatValueForInput(arrayOfObjects)).toBe("");
    // Asset objects keep their type/size summary instead of coercion.
    expect(formatValueForDisplay(asset)).not.toContain("[object Object]");
    expect(formatValueForDisplay([asset])).not.toContain("[object Object]");
  });
});
