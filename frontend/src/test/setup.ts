import "@testing-library/jest-dom/vitest";
import { afterAll, afterEach, beforeAll } from "vitest";

// Import MSW server - will be created when mocks are available
let server: ReturnType<typeof import("msw/node").setupServer>;

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
