// REQ-FE-044: Frontend multilingual dictionary and locale switching
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { ThemeMenu } from "./ThemeMenu";
import { locale, setLocale } from "~/lib/i18n";
import { initializeUiTheme, setColorMode, setPrimaryColor, setUiTheme } from "~/lib/ui-theme";

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

describe("ThemeMenu", () => {
	beforeEach(() => {
		setLocale("en");
	});
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

	it("switches UI theme", async () => {
		render(() => <ThemeMenu spaceId="default" />);
		const openButton = screen.getByRole("button", { name: /theme settings/i });
		await fireEvent.click(openButton);
		const classicRadio = screen.getByRole("radio", { name: "Classic" });
		await fireEvent.click(classicRadio);
		expect(document.documentElement.dataset.uiTheme).toBe("classic");
	});

	it("switches color mode", async () => {
		render(() => <ThemeMenu spaceId="default" />);
		const openButton = screen.getByRole("button", { name: /theme settings/i });
		await fireEvent.click(openButton);
		const darkRadio = screen.getByRole("radio", { name: /dark/i });
		await fireEvent.click(darkRadio);
		expect(document.documentElement.dataset.colorMode).toBe("dark");
	});

	it("switches primary color", async () => {
		render(() => <ThemeMenu spaceId="default" />);
		const openButton = screen.getByRole("button", { name: /theme settings/i });
		await fireEvent.click(openButton);
		const blueRadio = screen.getByRole("radio", { name: /blue/i });
		await fireEvent.click(blueRadio);
		expect(document.documentElement.dataset.primaryColor).toBe("blue");
	});

	it("closes on Escape key", async () => {
		render(() => <ThemeMenu spaceId="default" />);
		const openButton = screen.getByRole("button", { name: /theme settings/i });
		await fireEvent.click(openButton);
		expect(screen.getByText("Language")).toBeInTheDocument();
		await fireEvent.keyDown(document, { key: "Escape" });
		expect(screen.queryByText("Language")).not.toBeInTheDocument();
	});

	it("closes on outside click", async () => {
		render(() => <ThemeMenu spaceId="default" />);
		const openButton = screen.getByRole("button", { name: /theme settings/i });
		await fireEvent.click(openButton);
		expect(screen.getByText("Language")).toBeInTheDocument();
		await fireEvent.pointerDown(document.body);
		expect(screen.queryByText("Language")).not.toBeInTheDocument();
	});

	it("stays open when clicking inside the menu", async () => {
		render(() => <ThemeMenu spaceId="default" />);
		const openButton = screen.getByRole("button", { name: /theme settings/i });
		await fireEvent.click(openButton);
		const langText = screen.getByText("Language");
		await fireEvent.pointerDown(langText);
		expect(screen.getByText("Language")).toBeInTheDocument();
	});

	it("does not close menu on non-Escape key", async () => {
		render(() => <ThemeMenu spaceId="default" />);
		const openButton = screen.getByRole("button", { name: /theme settings/i });
		await fireEvent.click(openButton);
		await fireEvent.keyDown(document, { key: "Enter" });
		expect(screen.getByText("Language")).toBeInTheDocument();
	});
});

describe("initializeUiTheme", () => {
	it("applies theme attributes to document", () => {
		setUiTheme("materialize");
		setColorMode("light");
		setPrimaryColor("violet");
		initializeUiTheme();
		expect(document.documentElement.dataset.uiTheme).toBe("materialize");
		expect(document.documentElement.dataset.colorMode).toBe("light");
		expect(document.documentElement.dataset.primaryColor).toBe("violet");
	});
});
