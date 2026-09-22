import { afterEach, describe, expect, it, vi } from "vitest";
import { getDocsiteHref } from "./docsite-links";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("docsite links", () => {
  it("REQ-FE-056: auth guidance defaults to the published docsite outside development", () => {
    expect(
      getDocsiteHref(
        "/docs/develop/development-setup",
        "docs/develop/development-setup.md",
        {
          NODE_ENV: "test",
        },
      ),
    ).toBe(
      "https://ugoite.github.io/ugoite/docs/develop/development-setup",
    );
  });

  it("REQ-FE-056: auth guidance uses the local docsite origin during development", () => {
    expect(
      getDocsiteHref(
        "docs/operate/install-deploy",
        "docs/operate/install-deploy.md",
        {
          NODE_ENV: "development",
        },
      ),
    ).toBe("http://localhost:4321/docs/operate/install-deploy");
  });

  it("REQ-FE-017: doc links honor explicit docsite origins and GitHub fallbacks when docsite is unavailable", () => {
    expect(
      getDocsiteHref(
        "/docs/operate/storage-recovery",
        "docs/operate/storage-recovery.md",
        {
          DOCSITE_ORIGIN: "https://docs.example.test/base/",
        },
      ),
    ).toBe(
      "https://docs.example.test/base/docs/operate/storage-recovery",
    );
    expect(
      getDocsiteHref(
        "/docs/operate/storage-recovery",
        "docs/operate/storage-recovery.md",
        {
          DOCSITE_ORIGIN: "",
        },
      ),
    ).toBe(
      "https://github.com/ugoite/ugoite/blob/main/docs/operate/storage-recovery.md",
    );
    expect(
      getDocsiteHref(
        "docs/operate/storage-recovery",
        undefined,
        {
          DOCSITE_ORIGIN: "",
        },
      ),
    ).toBe("/docs/operate/storage-recovery");
  });

  it("REQ-FE-056: quickstart walkthrough honors the local origin during development", () => {
    expect(
      getDocsiteHref(
        "/docs/get-started/quickstart",
        "docs/get-started/quickstart.mdx",
        {
          NODE_ENV: "development",
        },
      ),
    ).toBe("http://localhost:4321/docs/get-started/quickstart");
    expect(
      getDocsiteHref(
        "/docs/get-started/quickstart",
        "docs/get-started/quickstart.mdx",
        {
          NODE_ENV: "test",
        },
      ),
    ).toBe(
      "https://ugoite.github.io/ugoite/docs/get-started/quickstart",
    );
  });

  it("REQ-FE-056: auth guidance still resolves when process env is unavailable", () => {
    vi.stubGlobal("process", undefined);
    expect(getDocsiteHref("/docs/develop/development-setup")).toBe(
      "https://ugoite.github.io/ugoite/docs/develop/development-setup",
    );
  });
});
