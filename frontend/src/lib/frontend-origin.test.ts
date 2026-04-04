// REQ-OPS-015: Frontend test origin selection follows explicit environment configuration.
import { afterEach, describe, expect, it, vi } from "vitest";
import {
	getFrontendApiBase,
	getFrontendTestApiBase,
	getFrontendTestOrigin,
	getRuntimeFrontendApiBase,
	getRuntimeFrontendOrigin,
} from "./frontend-origin";

describe("frontend origin helpers", () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.unstubAllEnvs();
	});

	it("REQ-OPS-015: falls back to the default frontend test origin when process env is unavailable", () => {
		vi.stubGlobal("process", undefined);

		expect(getFrontendTestOrigin()).toBe("http://localhost:3000");
		expect(getFrontendTestApiBase()).toBe("http://localhost:3000/api");
	});

	it("REQ-OPS-015: trims and applies FRONTEND_TEST_ORIGIN overrides", () => {
		const env = {
			FRONTEND_TEST_ORIGIN: " http://127.0.0.1:4310/ ",
		};

		expect(getFrontendTestOrigin(env)).toBe("http://127.0.0.1:4310");
		expect(getFrontendApiBase(getFrontendTestOrigin(env))).toBe("http://127.0.0.1:4310/api");
	});

	it("REQ-OPS-015: prefers runtime frontend origin env vars in documented order", () => {
		const env = {
			FRONTEND_ORIGIN: " https://front.example.test/ ",
			ORIGIN: "https://origin.example.test",
			FRONTEND_URL: "https://url.example.test",
		};

		expect(getRuntimeFrontendOrigin(env)).toBe("https://front.example.test");
		expect(getRuntimeFrontendApiBase(env)).toBe("https://front.example.test/api");
	});
});
