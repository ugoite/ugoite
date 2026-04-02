// REQ-FE-010: HelloWorld backend integration component
import "@testing-library/jest-dom/vitest";
import { describe, it, expect } from "vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { http, HttpResponse } from "msw";
import HelloWorld from "./HelloWorld";
import { server } from "~/test/mocks/server";
import { testApiUrl } from "~/test/http-origin";

describe("HelloWorld", () => {
	it("displays message from backend", async () => {
		server.use(
			http.get(testApiUrl("/"), () => HttpResponse.json({ message: "Hello from backend!" })),
		);
		render(() => <HelloWorld />);
		await waitFor(() => {
			expect(screen.getByText("Hello from backend!")).toBeInTheDocument();
		});
	});

	it("shows fallback on missing message", async () => {
		server.use(http.get(testApiUrl("/"), () => HttpResponse.json({})));
		render(() => <HelloWorld />);
		await waitFor(() => {
			expect(screen.getByText("No message")).toBeInTheDocument();
		});
	});

	it("shows error message on fetch failure", async () => {
		server.use(http.get(testApiUrl("/"), () => HttpResponse.error()));
		render(() => <HelloWorld />);
		await waitFor(() => {
			expect(screen.getByText("Error fetching")).toBeInTheDocument();
		});
	});
});
