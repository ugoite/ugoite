import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AboutRoute from "./about";
import HomeRoute from "./index";
import { setLocale } from "~/lib/i18n";

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

describe("public page localization", () => {
	beforeEach(() => {
		localStorage.clear();
		setLocale("en");
	});

	it("REQ-FE-064: home page localizes public copy through the shared dictionary", () => {
		render(() => <HomeRoute />);

		expect(
			screen.getByText("Local-first knowledge, structured for search and automation"),
		).toBeInTheDocument();

		setLocale("ja");

		return waitFor(() => {
			expect(
				screen.getByText("ローカルファーストの知識を、検索と自動化のために構造化"),
			).toBeInTheDocument();
			expect(screen.getByRole("link", { name: "ログイン" })).toHaveAttribute("href", "/login");
			expect(screen.getByText("まずはログインから始めてください。/spaces には認証済みのブラウザーセッションが必要です。")).toBeInTheDocument();
			expect(screen.getByRole("link", { name: "詳しく見る" })).toHaveAttribute("href");
			expect(
				screen.queryByText("Local-first knowledge, structured for search and automation"),
			).not.toBeInTheDocument();
			expect(
				screen.queryByText(
					"Start with Log in. /spaces requires an authenticated browser session.",
				),
			).not.toBeInTheDocument();
		});
	});

	it("REQ-FE-064: about page localizes public copy through the shared dictionary", () => {
		render(() => <AboutRoute />);

		expect(screen.getByRole("heading", { name: "About Ugoite" })).toBeInTheDocument();

		setLocale("ja");

		return waitFor(() => {
			expect(screen.getByRole("heading", { name: "Ugoite について" })).toBeInTheDocument();
			expect(screen.getByRole("link", { name: "ホームに戻る" })).toHaveAttribute("href", "/");
			expect(screen.getByText("技術スタック")).toBeInTheDocument();
			expect(screen.queryByText("About Ugoite")).not.toBeInTheDocument();
		});
	});
});
