import { describe, expect, it } from "vitest";
import { pwaUpdatePolicy } from "./pwa-update-policy.ts";

describe("PWA update policy", () => {
  it("REQ-E2E-010: update policy waits for existing clients", () => {
    expect(pwaUpdatePolicy.registerType).toBe("prompt");
    expect(pwaUpdatePolicy.workbox.skipWaiting).toBe(false);
    expect(pwaUpdatePolicy.workbox.clientsClaim).toBe(true);
    expect(pwaUpdatePolicy.workbox.navigateFallback).toBeNull();
  });
});
