import { describe, expect, it } from "vitest";
import {
  buildLoginPath,
  clearPendingLoginPath,
  consumePendingLoginPath,
  getCurrentPath,
  getSafeNextPath,
  isProtectedRoute,
  isPublicRoute,
  rememberPendingLoginPath,
} from "./auth-route";

describe("frontend authentication routes", () => {
  it("keeps the public route allowlist explicit", () => {
    expect(isPublicRoute("/")).toBe(true);
    expect(isPublicRoute("/login")).toBe(true);
    expect(isPublicRoute("/recover")).toBe(true);
    expect(isPublicRoute("/setup")).toBe(true);
    expect(isPublicRoute("/about")).toBe(true);
    expect(isPublicRoute("/spaces/join")).toBe(true);
    expect(isPublicRoute("/spaces/demo/dashboard")).toBe(false);
    expect(isPublicRoute("/device")).toBe(false);
    expect(isPublicRoute("/settings/security")).toBe(false);
    expect(isPublicRoute("/LOGIN/")).toBe(true);
    expect(isPublicRoute("/spaces//join/")).toBe(true);
    expect(isProtectedRoute("/spaces")).toBe(true);
    expect(isProtectedRoute("/spaces/demo/dashboard")).toBe(true);
    expect(isProtectedRoute("/settings/security")).toBe(true);
    expect(isProtectedRoute("/device")).toBe(true);
    expect(isProtectedRoute("/SPACES/")).toBe(true);
    expect(isProtectedRoute("/spaces//demo/dashboard/")).toBe(true);
    expect(isProtectedRoute("/SETTINGS/SECURITY/")).toBe(true);
    expect(isProtectedRoute("/device/")).toBe(true);
    expect(isProtectedRoute("/does-not-exist")).toBe(false);
  });

  it("preserves the requested path and query in the login redirect", () => {
    expect(getCurrentPath("/spaces/demo/dashboard", "?tab=recent")).toBe(
      "/spaces/demo/dashboard?tab=recent",
    );
    expect(buildLoginPath("/spaces/demo/dashboard", "?tab=recent")).toBe(
      "/login?next=%2Fspaces%2Fdemo%2Fdashboard%3Ftab%3Drecent",
    );
  });

  it("accepts only root-relative next paths", () => {
    expect(getSafeNextPath("/spaces/demo/dashboard?tab=recent")).toBe(
      "/spaces/demo/dashboard?tab=recent",
    );
    expect(getSafeNextPath("https://attacker.invalid/steal")).toBe("/spaces");
    expect(getSafeNextPath("//attacker.invalid/steal")).toBe("/spaces");
    expect(getSafeNextPath("spaces/demo")).toBe("/spaces");
    expect(getSafeNextPath(undefined)).toBe("/spaces");
  });

  it("stores and consumes a safe OIDC continuation path once", () => {
    sessionStorage.clear();
    rememberPendingLoginPath("/spaces/demo/dashboard?tab=recent");

    expect(consumePendingLoginPath()).toBe(
      "/spaces/demo/dashboard?tab=recent",
    );
    expect(consumePendingLoginPath()).toBeUndefined();
  });

  it("can clear a stale OIDC continuation before a new login method", () => {
    rememberPendingLoginPath("/spaces/demo/dashboard");
    clearPendingLoginPath();

    expect(consumePendingLoginPath()).toBeUndefined();
  });
});
