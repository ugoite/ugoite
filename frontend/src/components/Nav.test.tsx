// REQ-FE-010: Nav component
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import Nav from "./Nav";

let mockPathname = "/";

vi.mock("@solidjs/router", () => ({
	useLocation: () => ({ pathname: mockPathname }),
}));

describe("Nav", () => {
	it("renders navigation links", () => {
		mockPathname = "/";
		render(() => <Nav />);
		expect(screen.getByText("Home")).toBeInTheDocument();
		expect(screen.getByText("Spaces")).toBeInTheDocument();
		expect(screen.getByText("About")).toBeInTheDocument();
	});

	it("applies active class to current path", () => {
		mockPathname = "/";
		render(() => <Nav />);
		const homeLink = screen.getByText("Home").closest("a");
		expect(homeLink).toHaveClass("ui-nav-link-active");
	});

	it("returns null on space explorer pages", () => {
		mockPathname = "/spaces/my-space/entries";
		const { container } = render(() => <Nav />);
		expect(container.firstChild).toBeNull();
	});

	it("shows nav on spaces list page", () => {
		mockPathname = "/spaces";
		render(() => <Nav />);
		expect(screen.getByText("Spaces")).toBeInTheDocument();
	});

	it("applies active class to about page", () => {
		mockPathname = "/about";
		render(() => <Nav />);
		const aboutLink = screen.getByText("About").closest("a");
		expect(aboutLink).toHaveClass("ui-nav-link-active");
	});
});
