import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { SearchBar } from "./SearchBar";

describe("SearchBar", () => {
	it("should render search input", () => {
		render(() => <SearchBar onSearch={vi.fn()} />);
		expect(screen.getByPlaceholderText(/search/i)).toBeInTheDocument();
	});

	it("should call onSearch when form is submitted", async () => {
		const onSearch = vi.fn();
		render(() => <SearchBar onSearch={onSearch} />);

		const input = screen.getByPlaceholderText(/search/i);
		const form = screen.getByRole("search");

		fireEvent.input(input, { target: { value: "test query" } });
		fireEvent.submit(form);

		expect(onSearch).toHaveBeenCalledWith("test query");
	});

	it("should not submit empty search", () => {
		const onSearch = vi.fn();
		render(() => <SearchBar onSearch={onSearch} />);

		const form = screen.getByRole("search");
		fireEvent.submit(form);

		expect(onSearch).not.toHaveBeenCalled();
	});

	it("should clear search when clear button is clicked", () => {
		const onSearch = vi.fn();
		render(() => <SearchBar onSearch={onSearch} />);

		const input = screen.getByPlaceholderText(/search/i) as HTMLInputElement;
		fireEvent.input(input, { target: { value: "test" } });

		const clearButton = screen.getByLabelText(/clear/i);
		fireEvent.click(clearButton);

		expect(input.value).toBe("");
	});

	it("should display loading state", () => {
		render(() => <SearchBar onSearch={vi.fn()} loading={true} />);
		expect(screen.getByText(/searching/i)).toBeInTheDocument();
	});

	it("should display search results count", () => {
		render(() => <SearchBar onSearch={vi.fn()} resultsCount={5} />);
		expect(screen.getByText(/5 results/i)).toBeInTheDocument();
	});

	it("should allow keyboard shortcut (Cmd/Ctrl+K)", () => {
		const onSearch = vi.fn();
		render(() => <SearchBar onSearch={onSearch} />);

		const input = screen.getByPlaceholderText(/search/i);

		// Simulate Cmd+K or Ctrl+K
		fireEvent.keyDown(document, { key: "k", metaKey: true });
		expect(input).toHaveFocus();
	});
});
