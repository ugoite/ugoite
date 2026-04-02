import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import LoginRoute from "./login";
import { clearAuthTokenCookie, setAuthTokenCookie } from "~/lib/auth-session";
import { resetMockData, seedDevAuthConfig } from "~/test/mocks/handlers";

const navigateMock = vi.fn();
const localDevAuthGuideUrl =
	"https://github.com/ugoite/ugoite/blob/main/docs/guide/local-dev-auth-login.md";

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
		clearAuthTokenCookie();
	});

	it("REQ-OPS-015: signs in with passkey-totp and stores a browser auth cookie", async () => {
		seedDevAuthConfig({
			mode: "passkey-totp",
			username_hint: "dev-alice",
			supports_passkey_totp: true,
			supports_mock_oauth: false,
		});

		render(() => <LoginRoute />);

		expect(await screen.findByDisplayValue("dev-alice")).toBeInTheDocument();
		fireEvent.input(screen.getByLabelText("2FA code"), {
			target: { value: "123456" },
		});
		const form = screen.getByRole("button", { name: "Sign in with passkey + 2FA" }).closest("form");
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
			supports_passkey_totp: false,
			supports_mock_oauth: true,
		});

		render(() => <LoginRoute />);

		fireEvent.click(await screen.findByRole("button", { name: "Continue with Mock OAuth" }));

		await waitFor(() => {
			expect(navigateMock).toHaveBeenCalledWith("/spaces", { replace: true });
		});
		expect(document.cookie).toContain("ugoite_auth_bearer_token=frontend-test-token");
	});

	it("REQ-OPS-015: visiting login preserves an existing browser auth cookie until re-auth completes", async () => {
		seedDevAuthConfig({
			mode: "mock-oauth",
			username_hint: "dev-oauth-user",
			supports_passkey_totp: false,
			supports_mock_oauth: true,
		});
		setAuthTokenCookie("existing-browser-session", 1_900_000_000);

		render(() => <LoginRoute />);

		await screen.findByRole("button", { name: "Continue with Mock OAuth" });
		expect(document.cookie).toContain("ugoite_auth_bearer_token=existing-browser-session");
		expect(navigateMock).not.toHaveBeenCalled();
	});

	it("REQ-OPS-015: shows first-run passkey guidance with the canonical local auth guide", async () => {
		seedDevAuthConfig({
			mode: "passkey-totp",
			username_hint: "dev-alice",
			supports_passkey_totp: true,
			supports_mock_oauth: false,
		});

		render(() => <LoginRoute />);

		expect(await screen.findByRole("heading", { name: "First time here?" })).toBeInTheDocument();
		expect(screen.getByText(/Start the local stack with/i)).toBeInTheDocument();
		expect(screen.getByText("mise run dev")).toBeInTheDocument();
		expect(
			screen.getByText(/Generate the current 2FA code from your authenticator or the guide's/i),
		).toBeInTheDocument();
		expect(screen.getByText("oathtool")).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Local Dev Auth/Login" })).toHaveAttribute(
			"href",
			localDevAuthGuideUrl,
		);
	});

	it("REQ-OPS-015: keeps first-run passkey guidance out of mock-oauth login", async () => {
		seedDevAuthConfig({
			mode: "mock-oauth",
			username_hint: "dev-oauth-user",
			supports_passkey_totp: false,
			supports_mock_oauth: true,
		});

		render(() => <LoginRoute />);

		await screen.findByRole("button", { name: "Continue with Mock OAuth" });
		expect(screen.queryByRole("heading", { name: "First time here?" })).not.toBeInTheDocument();
		expect(screen.queryByRole("link", { name: "Local Dev Auth/Login" })).not.toBeInTheDocument();
	});
});
