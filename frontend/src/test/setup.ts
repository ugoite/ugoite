import "@testing-library/jest-dom/vitest";
import { isCommonAssetRequest } from "msw";
import { afterAll, afterEach, beforeAll, vi } from "vitest";
import { HttpNetworkFrame } from "msw/experimental";
import { setLocale } from "../lib/i18n";

// Keep Vitest's generated `?init` loader on the checked WASM artifact without
// asking its URL fetch to go through the local frontend origin. MSW continues
// to own API request interception; the protocol tests still execute the real
// compiled module.
vi.mock("../lib/generated/ugoite_wasm.wasm?init", () => ({
  default: async () => {
    const { readFile } = await import("node:fs/promises");
    const bytes = await readFile("src/lib/generated/ugoite_wasm.wasm");
    const { instance } = await WebAssembly.instantiate(bytes);
    return instance;
  },
}));

// Import MSW server - will be created when mocks are available
let server: ReturnType<typeof import("msw/node").setupServer>;

const createStorageMock = () => {
  const data = new Map<string, string>();
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, String(value));
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
    key: (index: number) => Array.from(data.keys())[index] ?? null,
    get length() {
      return data.size;
    },
  };
};

const ensureStorage = (name: "localStorage" | "sessionStorage") => {
  const storage = globalThis[name];
  if (storage && typeof storage.getItem === "function") return;
  const mock = createStorageMock();
  Object.defineProperty(globalThis, name, {
    configurable: true,
    value: mock,
  });
  if (typeof window !== "undefined") {
    Object.defineProperty(window, name, {
      configurable: true,
      value: mock,
    });
  }
};

ensureStorage("localStorage");
ensureStorage("sessionStorage");

beforeAll(async () => {
  const { server: mswServer } = await import("./mocks/server");
  server = mswServer;
  server.listen({
    onUnhandledFrame: async ({ frame, defaults }) => {
      if (frame instanceof HttpNetworkFrame) {
        if (!isCommonAssetRequest(frame.data.request)) {
          throw new Error(await frame.getUnhandledMessage());
        }
        return;
      }
      defaults.warn();
    },
  });
});

// Reset handlers after each test
afterEach(() => {
  server?.resetHandlers();
  setLocale("en");
});

// Close server after all tests
afterAll(() => {
  server?.close();
});
