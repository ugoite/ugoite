// REQ-FE-066: Auth-aware global navigation
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createRoot } from "solid-js";
import Nav from "./Nav";

let mockPathname = "/";
const navigateMock = vi.fn();
const getSessionMock = vi.fn();
const clearSessionMock = vi.fn();

vi.mock("@solidjs/router", () => ({
	useLocation: () => ({ pathname: mockPathname }),
	useNavigate: () => navigateMock,
}));

vi.mock("~/lib/auth-api", () => ({
	authApi: {
		getSession: (...args: unknown[]) => getSessionMock(...args),
		clearSession: (...args: unknown[]) => clearSessionMock(...args),
	},
}));

describe("Nav", () => {
	afterEach(() => {
		vi.unstubAllGlobals();
	});

	beforeEach(() => {
		mockPathname = "/";
		navigateMock.mockReset();
		getSessionMock.mockReset();
		clearSessionMock.mockReset();
		getSessionMock.mockResolvedValue({ authenticated: false });
		clearSessionMock.mockResolvedValue(undefined);
	});

	it("REQ-FE-066: signed-out nav shows primary routes and a login link", async () => {
		mockPathname = "/";
		render(() => <Nav />);
		expect(screen.getByText("Home")).toBeInTheDocument();
		expect(screen.getByText("Spaces")).toBeInTheDocument();
		expect(screen.getByText("About")).toBeInTheDocument();
		expect(await screen.findByRole("link", { name: "Login" })).toHaveAttribute("href", "/login");
		expect(screen.queryByText("Signed in")).not.toBeInTheDocument();
		expect(screen.queryByRole("button", { name: "Sign out" })).not.toBeInTheDocument();
	});

	it("REQ-FE-066: active state tracks the current route for signed-out nav links", async () => {
		mockPathname = "/login";
		render(() => <Nav />);
		const loginLink = await screen.findByRole("link", { name: "Login" });
		expect(loginLink).toHaveClass("ui-nav-link-active");
	});

	it("REQ-FE-066: stays hidden on space explorer pages", () => {
		mockPathname = "/spaces/my-space/entries";
		const { container } = render(() => <Nav />);
		expect(container.firstChild).toBeNull();
	});

	it("REQ-FE-066: signed-in nav swaps Login for session status and sign-out", async () => {
		getSessionMock.mockResolvedValue({ authenticated: true });
		mockPathname = "/spaces";
		render(() => <Nav />);
		expect(screen.getByText("Spaces")).toBeInTheDocument();
		expect(await screen.findByText("Signed in")).toBeInTheDocument();
		expect(await screen.findByRole("button", { name: "Sign out" })).toBeInTheDocument();
		await waitFor(() => {
			expect(screen.queryByRole("link", { name: "Login" })).not.toBeInTheDocument();
		});
	});

	it("REQ-FE-066: sign-out clears the auth cookie and redirects to login", async () => {
		getSessionMock.mockResolvedValue({ authenticated: true });
		mockPathname = "/spaces";
		render(() => <Nav />);

		fireEvent.click(await screen.findByRole("button", { name: "Sign out" }));

		await waitFor(() => {
			expect(clearSessionMock).toHaveBeenCalledTimes(1);
		});
		await waitFor(() => {
			expect(navigateMock).toHaveBeenCalledWith("/login", { replace: true });
		});
	});

	it("REQ-FE-066: safely initializes without a browser window during SSR", () => {
		vi.stubGlobal("window", undefined);
		mockPathname = "/";

		expect(() => {
			const dispose = createRoot((dispose) => {
				Nav();
				return dispose;
			});
			dispose();
		}).not.toThrow();
	});
});
