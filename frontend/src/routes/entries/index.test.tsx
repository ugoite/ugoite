import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import EntriesIndexPane from "./index";

vi.mock("@solidjs/router", () => ({
	Navigate: (props: { href: string }) => <div data-testid="navigate" data-href={props.href} />,
}));

describe("REQ-FE-032: legacy entries route redirects", () => {
	it("redirects to /spaces", () => {
		render(() => <EntriesIndexPane />);
		expect(screen.getByTestId("navigate")).toHaveAttribute("data-href", "/spaces");
	});
});
