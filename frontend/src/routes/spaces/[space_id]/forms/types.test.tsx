import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formApi } from "~/lib/ugoite-client";
import SpaceFormTypesRoute from "./types";

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: { listTypes: vi.fn() },
}));

describe("form types route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
    vi.mocked(formApi.listTypes).mockResolvedValue(["string", "double"]);
  });

  it("renders field types returned by the Forms API", async () => {
    render(() => <SpaceFormTypesRoute />);

    expect(await screen.findByText("string")).toBeInTheDocument();
    expect(await screen.findByText("double")).toBeInTheDocument();
    expect(formApi.listTypes).toHaveBeenCalledWith("default");
  });
});
