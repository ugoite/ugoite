import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { http, HttpResponse } from "msw";
import LoginRoute from "./login";
import { clearAuthTokenCookie, setAuthTokenCookie } from "~/lib/auth-session";
import { resetMockData, seedDevAuthConfig } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";

const navigateMock = vi.fn();
const containerQuickStartGuideUrl =
	"https://ugoite.github.io/ugoite/docs/guide/container-quickstart";
const localDevAuthGuideUrl =
	"https://ugoite.github.io/ugoite/docs/guide/local-dev-auth-login";

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

	it("REQ-OPS-015: keeps the /spaces shortcut out of signed-out login screens", async () => {
		seedDevAuthConfig({
			mode: "mock-oauth",
			username_hint: "dev-oauth-user",
			supports_passkey_totp: false,
			supports_mock_oauth: true,
		});

		render(() => <LoginRoute />);

		await screen.findByRole("button", { name: "Continue with Mock OAuth" });
		expect(screen.getByRole("link", { name: "Back to Home" })).toHaveAttribute("href", "/");
		expect(screen.queryByRole("link", { name: "Go to Spaces" })).not.toBeInTheDocument();
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

	it("REQ-OPS-015: keeps auth-config recovery guidance compatible with source and published browser flows", async () => {
		server.use(
			http.get(
				"http://localhost:3000/api/auth/config",
				() => new HttpResponse(null, { status: 500, statusText: "Internal Server Error" }),
			),
		);

		render(() => <LoginRoute />);

		expect(
			await screen.findByText(/Confirm the Ugoite stack you started is still running/i),
		).toBeInTheDocument();
		expect(screen.getByText("mise run dev")).toBeInTheDocument();
		expect(
			screen.getByText(/Restart your Docker Compose services and reopen/i),
		).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Container Quick Start" })).toHaveAttribute(
			"href",
			containerQuickStartGuideUrl,
		);
		expect(screen.getByRole("link", { name: "Local Dev Auth/Login" })).toHaveAttribute(
			"href",
			localDevAuthGuideUrl,
		);
		expect(
			screen.queryByText("Failed to load the current auth mode. Re-run"),
		).not.toBeInTheDocument();
	});
});
