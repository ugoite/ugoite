// REQ-FE-066: Auth-aware global navigation
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createRoot } from "solid-js";
import Nav from "./Nav";
import { clearAuthTokenCookie, setAuthTokenCookie } from "~/lib/auth-session";

let mockPathname = "/";
const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
	useLocation: () => ({ pathname: mockPathname }),
	useNavigate: () => navigateMock,
}));

describe("Nav", () => {
	afterEach(() => {
		vi.unstubAllGlobals();
	});

	beforeEach(() => {
		mockPathname = "/";
		navigateMock.mockReset();
		clearAuthTokenCookie();
	});

	it("REQ-FE-066: signed-out nav shows primary routes and a login link", () => {
		mockPathname = "/";
		render(() => <Nav />);
		expect(screen.getByText("Home")).toBeInTheDocument();
		expect(screen.getByText("Spaces")).toBeInTheDocument();
		expect(screen.getByText("About")).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Login" })).toHaveAttribute("href", "/login");
		expect(screen.queryByText("Signed in")).not.toBeInTheDocument();
		expect(screen.queryByRole("button", { name: "Sign out" })).not.toBeInTheDocument();
	});

	it("REQ-FE-066: active state tracks the current route for signed-out nav links", () => {
		mockPathname = "/login";
		render(() => <Nav />);
		const loginLink = screen.getByRole("link", { name: "Login" });
		expect(loginLink).toHaveClass("ui-nav-link-active");
	});

	it("REQ-FE-066: stays hidden on space explorer pages", () => {
		mockPathname = "/spaces/my-space/entries";
		const { container } = render(() => <Nav />);
		expect(container.firstChild).toBeNull();
	});

	it("REQ-FE-066: signed-in nav swaps Login for session status and sign-out", async () => {
		setAuthTokenCookie("existing-browser-session", 1_900_000_000);
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
		setAuthTokenCookie("existing-browser-session", 1_900_000_000);
		mockPathname = "/spaces";
		render(() => <Nav />);

		fireEvent.click(await screen.findByRole("button", { name: "Sign out" }));

		expect(document.cookie).not.toContain("ugoite_auth_bearer_token=");
		expect(navigateMock).toHaveBeenCalledWith("/login", { replace: true });
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
