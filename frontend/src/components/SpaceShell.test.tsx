// REQ-FE-010: SpaceShell layout component
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import { SpaceShell } from "./SpaceShell";
import { loadingState } from "~/lib/loading";

vi.mock("@solidjs/router", () => ({
	A: (props: {
		href: string;
		class?: string;
		classList?: Record<string, boolean>;
		children: unknown;
	}) => {
		const classes = [
			props.class,
			...(props.classList ? Object.keys(props.classList).filter((k) => props.classList?.[k]) : []),
		]
			.filter(Boolean)
			.join(" ");
		return (
			<a href={props.href} class={classes}>
				{props.children}
			</a>
		);
	},
}));

describe("SpaceShell", () => {
	it("renders children", () => {
		render(() => (
			<SpaceShell spaceId="my-space">
				<div>Child content</div>
			</SpaceShell>
		));
		expect(screen.getByText("Child content")).toBeInTheDocument();
	});

	it("renders top navigation tabs", () => {
		render(() => (
			<SpaceShell spaceId="my-space">
				<div>Content</div>
			</SpaceShell>
		));
		const dashboardLink = screen.getByRole("link", { name: /dashboard/i });
		expect(dashboardLink).toHaveAttribute("href", "/spaces/my-space/dashboard");
	});

	it("applies active class to active top tab", () => {
		render(() => (
			<SpaceShell spaceId="my-space" activeTopTab="dashboard">
				<div>Content</div>
			</SpaceShell>
		));
		const dashboardLink = screen.getByRole("link", { name: /dashboard/i });
		expect(dashboardLink).toHaveClass("ui-tab-active");
	});

	it("renders bottom tabs when showBottomTabs is true", () => {
		render(() => (
			<SpaceShell spaceId="my-space" showBottomTabs={true} activeBottomTab="object">
				<div>Content</div>
			</SpaceShell>
		));
		expect(screen.getByRole("link", { name: /object/i })).toBeInTheDocument();
		expect(screen.getByRole("link", { name: /grid/i })).toBeInTheDocument();
	});

	it("does not render bottom tabs when showBottomTabs is false", () => {
		render(() => (
			<SpaceShell spaceId="my-space" showBottomTabs={false}>
				<div>Content</div>
			</SpaceShell>
		));
		expect(screen.queryByRole("link", { name: /object/i })).not.toBeInTheDocument();
	});

	it("applies bottomTabHrefSuffix to tab links", () => {
		render(() => (
			<SpaceShell spaceId="my-space" showBottomTabs={true} bottomTabHrefSuffix="?mode=grid">
				<div>Content</div>
			</SpaceShell>
		));
		const objectLink = screen.getByRole("link", { name: /object/i });
		expect(objectLink).toHaveAttribute("href", "/spaces/my-space/entries?mode=grid");
	});

	it("renders bottom tabs with active class", () => {
		render(() => (
			<SpaceShell spaceId="my-space" showBottomTabs={true} activeBottomTab="grid">
				<div>Content</div>
			</SpaceShell>
		));
		const gridLink = screen.getByRole("link", { name: /grid/i });
		expect(gridLink).toHaveClass("ui-tab-active");
	});

	it("shows loading bar when loading", () => {
		loadingState.start();
		render(() => (
			<SpaceShell spaceId="my-space">
				<div>Content</div>
			</SpaceShell>
		));
		expect(document.querySelector(".ui-loading-bar")).toBeInTheDocument();
		loadingState.stop();
	});
});
