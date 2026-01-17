import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import NotesIndexPane from "./index";

vi.mock("@solidjs/router", () => ({
	Navigate: (props: { href: string }) => <div data-testid="navigate" data-href={props.href} />,
}));

describe("REQ-FE-032: legacy notes route redirects", () => {
	it("redirects to /workspaces", () => {
		render(() => <NotesIndexPane />);
		expect(screen.getByTestId("navigate")).toHaveAttribute("data-href", "/workspaces");
	});
});
