import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import About from "./about";
import { setLocale } from "~/lib/i18n";

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

describe("/about", () => {
	beforeEach(() => {
		localStorage.clear();
		setLocale("en");
	});

	it("REQ-FE-044: localizes about route copy in Japanese", () => {
		render(() => <About />);

		expect(screen.getByText("FastAPI (Python 3.13+)")).toBeInTheDocument();
		setLocale("ja");

		expect(screen.getByRole("heading", { name: "Ugoite について" })).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "スペースを開く" })).toHaveAttribute("href", "/spaces");
		expect(screen.getByRole("link", { name: "ホームに戻る" })).toHaveAttribute("href", "/");
		expect(screen.getByText(/柔軟な構造と高速な検索/u)).toBeInTheDocument();
		expect(screen.getByText("ローカルファーストの所有権")).toBeInTheDocument();
		expect(screen.getByRole("heading", { name: "仕組み" })).toBeInTheDocument();
		expect(screen.getByText("技術スタック")).toBeInTheDocument();
		expect(screen.getByText("SolidStart + Tailwind CSS")).toBeInTheDocument();
		expect(screen.getByText("FastAPI (Python 3.13+)")).toBeInTheDocument();
	});
});
