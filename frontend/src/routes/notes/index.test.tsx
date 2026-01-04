import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, waitFor } from "@solidjs/testing-library";
import NotesIndexPage from "./index";
import { resetMockData, seedWorkspace } from "~/test/mocks/handlers";
import type { Workspace } from "~/lib/types";
import { schemaApi } from "~/lib/client";

vi.mock("@solidjs/router", () => ({
	useNavigate: () => vi.fn(),
}));

describe("/notes (index route)", () => {
	beforeEach(() => {
		resetMockData();
		const ws: Workspace = {
			id: "default",
			name: "Default",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedWorkspace(ws);
	});

	it("does not call schemaApi.list with empty workspaceId", async () => {
		const spy = vi.spyOn(schemaApi, "list");
		render(() => <NotesIndexPage />);

		await waitFor(() => {
			// Workspace store should eventually select a workspace
			expect(spy).toHaveBeenCalled();
		});

		for (const call of spy.mock.calls) {
			expect(call[0]).not.toBe("");
		}
	});
});
