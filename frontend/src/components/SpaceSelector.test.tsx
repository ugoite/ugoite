// REQ-FE-001: Space selector
// REQ-FE-002: Automatic default space creation
// REQ-FE-003: Persist space selection
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { SpaceSelector } from "./SpaceSelector";
import type { Space } from "~/lib/types";

describe("SpaceSelector", () => {
	const mockSpaces: Space[] = [
		{ id: "ws-1", name: "Space One", created_at: "2025-01-01T00:00:00Z" },
		{ id: "ws-2", name: "Space Two", created_at: "2025-01-01T00:00:00Z" },
	];

	it("should render space options", () => {
		const onSelect = vi.fn();

		render(() => (
			<SpaceSelector
				spaces={mockSpaces}
				selectedSpaceId="ws-1"
				loading={false}
				error={null}
				onSelect={onSelect}
			/>
		));

		const select = screen.getByRole("combobox");
		expect(select).toBeInTheDocument();

		const options = screen.getAllByRole("option");
		expect(options).toHaveLength(2);
		expect(options[0]).toHaveTextContent("Space One");
		expect(options[1]).toHaveTextContent("Space Two");
	});

	it("should show loading state", () => {
		const onSelect = vi.fn();

		render(() => (
			<SpaceSelector
				spaces={[]}
				selectedSpaceId={null}
				loading={true}
				error={null}
				onSelect={onSelect}
			/>
		));

		expect(screen.getByText("Loading...")).toBeInTheDocument();
	});

	it("should show error message", () => {
		const onSelect = vi.fn();

		render(() => (
			<SpaceSelector
				spaces={mockSpaces}
				selectedSpaceId="ws-1"
				loading={false}
				error="Failed to load"
				onSelect={onSelect}
			/>
		));

		expect(screen.getByText("Failed to load")).toBeInTheDocument();
	});

	it("should call onSelect when space changes", async () => {
		const onSelect = vi.fn();

		render(() => (
			<SpaceSelector
				spaces={mockSpaces}
				selectedSpaceId="ws-1"
				loading={false}
				error={null}
				onSelect={onSelect}
			/>
		));

		const select = screen.getByRole("combobox");
		await fireEvent.change(select, { target: { value: "ws-2" } });

		expect(onSelect).toHaveBeenCalledWith("ws-2");
	});
});
