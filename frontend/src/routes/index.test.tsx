import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import HomeRoute from "./index";

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

describe("home route", () => {
	beforeEach(() => {
		localStorage.clear();
		setLocale("en");
	});

	it("REQ-E2E-008: public home page routes Learn More to the canonical getting-started docsite flow", () => {
		render(() => <HomeRoute />);

		expect(screen.getByRole("heading", { name: "Ugoite" })).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Learn More" })).toHaveAttribute(
			"href",
			"https://ugoite.github.io/ugoite/getting-started",
		);
	});

	it("REQ-OPS-015: public home page points first-time users to /login before /spaces", () => {
		render(() => <HomeRoute />);

		const loginLink = screen.getByRole("link", { name: "Log in" });
		const spacesLink = screen.getByRole("link", { name: "Open Spaces" });

		expect(screen.getByRole("heading", { name: "Ugoite" })).toBeInTheDocument();
		expect(loginLink).toHaveAttribute("href", "/login");
		expect(loginLink).toHaveClass("ui-button-primary");
		expect(spacesLink).toHaveAttribute("href", "/spaces");
		expect(spacesLink).toHaveClass("ui-button-secondary");
		expect(screen.getByText(/Start with Log in\./i)).toHaveTextContent(
			"/spaces requires an authenticated browser session.",
		);
	});

	it("REQ-FE-044: localizes home route CTA copy in Japanese", () => {
		render(() => <HomeRoute />);
		setLocale("ja");

		expect(
			screen.getByText("ローカルファーストの知識を、検索と自動化のために構造化"),
		).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "ログイン" })).toHaveAttribute("href", "/login");
		expect(screen.getByRole("link", { name: "スペースを開く" })).toHaveAttribute("href", "/spaces");
		expect(screen.getByRole("link", { name: "詳しく見る" })).toHaveAttribute(
			"href",
			"https://ugoite.github.io/ugoite/getting-started",
		);
	});
});
