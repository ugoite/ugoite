import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import type { JSX } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import CompositionEditRoute from "./edit";

const { canonicalizeMock, getMock, lintMock, studioProps } = vi.hoisted(() => ({
  canonicalizeMock: vi.fn(),
  getMock: vi.fn(),
  lintMock: vi.fn(),
  studioProps: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  useParams: () => ({
    space_id: "space-1",
    composition_id: "tool-1",
    revision_id: "revision-2",
  }),
  A: (props: { href: string; class?: string; children: JSX.Element }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
}));

vi.mock("~/lib/composition-api", () => ({
  compositionApi: {
    canonicalizeDocument: (...args: unknown[]) => canonicalizeMock(...args),
    get: (...args: unknown[]) => getMock(...args),
    lint: (...args: unknown[]) => lintMock(...args),
  },
}));

vi.mock("~/components/CompositionStudio", () => ({
  CompositionStudio: (props: Record<string, unknown>) => {
    studioProps(props);
    return <div data-testid="graph-studio" />;
  },
}));

const graphDocument = {
  format: "ugoite.composition",
  format_version: 1,
  kind: "dashboard",
  name: "Monthly report",
  spec: {
    variables: [],
    components: [],
    layout: { kind: "flow", rows: [] },
  },
};

const linted = (document: unknown) => ({
  ok: true,
  value: { document, canonical_yaml: "canonical", fingerprint: "fp" },
});

describe("Composition edit route", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    getMock.mockResolvedValue({ fields: { spec: "strict graph yaml" } });
    lintMock.mockImplementation(async () => linted(graphDocument));
    canonicalizeMock.mockImplementation(async (document: unknown) => ({
      document,
      canonical_yaml: "canonical",
      fingerprint: "fp",
    }));
  });

  it("loads a strict variable graph into the editor", async () => {
    render(() => <CompositionEditRoute />);

    expect(await screen.findByTestId("graph-studio")).toBeInTheDocument();
    expect(studioProps).toHaveBeenCalledWith(expect.objectContaining({
      spaceId: "space-1",
      initialDraft: expect.objectContaining({
        name: "Monthly report",
        variables: [],
        components: [],
      }),
      saveMode: {
        kind: "update",
        compositionId: "tool-1",
        baseRevisionId: "revision-2",
      },
    }));
  });

  it("rejects a document containing the removed sources shape", async () => {
    lintMock.mockImplementation(async () =>
      linted({
        ...graphDocument,
        spec: { ...graphDocument.spec, sources: [] },
      })
    );
    render(() => <CompositionEditRoute />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This revision cannot be edited.",
    );
    expect(screen.queryByTestId("graph-studio")).toBeNull();
  });
});
