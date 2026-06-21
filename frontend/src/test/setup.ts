import "@testing-library/jest-dom/vitest";
import { afterAll, afterEach, beforeAll } from "vitest";

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
  server.listen({ onUnhandledRequest: "error" });
});

// Reset handlers after each test
afterEach(() => {
  server?.resetHandlers();
});

// Close server after all tests
afterAll(() => {
  server?.close();
});
