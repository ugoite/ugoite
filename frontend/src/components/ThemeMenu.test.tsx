// REQ-FE-044: Frontend multilingual dictionary and locale switching
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { ThemeMenu } from "./ThemeMenu";
import { locale } from "~/lib/i18n";

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

describe("ThemeMenu", () => {
	it("shows language options in settings menu", async () => {
		render(() => <ThemeMenu spaceId="default" />);

		const openButton = screen.getByRole("button", { name: /theme settings/i });
		await fireEvent.click(openButton);

		expect(screen.getByText("Language")).toBeInTheDocument();
		expect(screen.getByRole("radio", { name: "English" })).toBeInTheDocument();
		expect(screen.getByRole("radio", { name: "日本語" })).toBeInTheDocument();
	});

	it("switches locale when language changes", async () => {
		render(() => <ThemeMenu spaceId="default" />);

		const openButton = screen.getByRole("button", { name: /theme settings/i });
		await fireEvent.click(openButton);

		await fireEvent.click(screen.getByRole("radio", { name: "日本語" }));

		expect(locale()).toBe("ja");
		expect(document.documentElement.lang).toBe("ja");
	});
});
