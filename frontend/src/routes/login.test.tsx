import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import LoginRoute from "./login";
import { resetMockData, seedDevAuthConfig } from "~/test/mocks/handlers";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
	useNavigate: () => navigateMock,
	useSearchParams: () => [{ next: "/spaces" }],
}));

describe("/login", () => {
	beforeEach(() => {
		navigateMock.mockReset();
		resetMockData();
		document.cookie = "ugoite_auth_bearer_token=; Path=/; Max-Age=0; SameSite=Lax";
	});

	it("REQ-OPS-015: signs in with manual-totp and stores a browser auth cookie", async () => {
		seedDevAuthConfig({
			mode: "manual-totp",
			username_hint: "dev-alice",
			supports_manual_totp: true,
			supports_mock_oauth: false,
		});

		render(() => <LoginRoute />);

		expect(await screen.findByDisplayValue("dev-alice")).toBeInTheDocument();
		fireEvent.input(screen.getByLabelText("2FA code"), {
			target: { value: "123456" },
		});
		const form = screen.getByRole("button", { name: "Sign in with 2FA" }).closest("form");
		expect(form).not.toBeNull();
		if (form === null) {
			throw new Error("Manual login form should exist.");
		}
		fireEvent.submit(form);

		await waitFor(() => {
			expect(navigateMock).toHaveBeenCalledWith("/spaces", { replace: true });
		});
		expect(document.cookie).toContain("ugoite_auth_bearer_token=frontend-test-token");
	});

	it("REQ-OPS-015: uses explicit mock-oauth browser login without startup auth injection", async () => {
		seedDevAuthConfig({
			mode: "mock-oauth",
			username_hint: "dev-oauth-user",
			supports_manual_totp: false,
			supports_mock_oauth: true,
		});

		render(() => <LoginRoute />);

		fireEvent.click(await screen.findByRole("button", { name: "Continue with Mock OAuth" }));

		await waitFor(() => {
			expect(navigateMock).toHaveBeenCalledWith("/spaces", { replace: true });
		});
		expect(document.cookie).toContain("ugoite_auth_bearer_token=frontend-test-token");
	});
});
