import { describe, expect, it } from "vitest";
import { isServer, render } from "solid-js/web";

describe("Solid runtime", () => {
  it("uses the client renderer in Vitest", () => {
    expect(isServer).toBe(false);
    expect(typeof render).toBe("function");
  });
});
