import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import SpaceTestConnectionRoute from "./test-connection";
import { expectBackLinkAtHeaderStart } from "~/test/back-link-placement";

const { getSpaceMock } = vi.hoisted(() => ({ getSpaceMock: vi.fn() }));

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
      {props.children as never}
    </a>
  ),
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  spaceApi: {
    get: (...args: unknown[]) => getSpaceMock(...args),
    testConnection: vi.fn(),
  },
}));

describe("SpaceTestConnectionRoute", () => {
  beforeEach(() => {
    setLocale("en");
    getSpaceMock.mockReset().mockResolvedValue({
      storage_config: { uri: "file:///tmp/space" },
    });
  });

  it("places its single storage-settings back link at the header start", async () => {
    render(() => <SpaceTestConnectionRoute />);

    const back = await screen.findByRole("link", {
      name: "Back to Storage settings",
    });
    expect(back).toHaveAttribute(
      "href",
      "/spaces/default/settings?section=storage",
    );
    expect(screen.getAllByRole("link", {
      name: "Back to Storage settings",
    })).toHaveLength(1);
    expectBackLinkAtHeaderStart(back);
  });
});
