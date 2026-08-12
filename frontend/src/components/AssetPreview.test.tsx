import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";

import { AssetPreview } from "./AssetPreview";
import type { AssetReference } from "~/lib/types";

const asset = (name: string, media_type: string): AssetReference => ({
  asset_id: `asset-${name}`,
  name,
  media_type,
  size_bytes: 10,
  sha256: "a".repeat(64),
});

describe("AssetPreview", () => {
  it("keeps Markdown content escaped while rendering the existing preview syntax", async () => {
    render(() => (
      <AssetPreview
        reference={asset("readme.md", "text/markdown")}
        blob={new Blob(["# Hello\n\n<script>alert(1)</script>"])}
        url="blob:markdown"
      />
    ));

    const preview = await screen.findByText("Hello");
    expect(preview).toBeInTheDocument();
    expect(document.querySelector("script")).toBeNull();
    expect(screen.getByText("<script>alert(1)</script>")).toBeInTheDocument();
  });

  it("renders native media and document elements from an authorized object URL", () => {
    const { container } = render(() => (
      <>
        <AssetPreview
          reference={asset("photo.png", "image/png")}
          blob={new Blob(["image"])}
          url="blob:image"
        />
        <AssetPreview
          reference={asset("report.pdf", "application/pdf")}
          blob={new Blob(["pdf"])}
          url="blob:pdf"
        />
        <AssetPreview
          reference={asset("recording.mp3", "audio/mpeg")}
          blob={new Blob(["audio"])}
          url="blob:audio"
        />
        <AssetPreview
          reference={asset("clip.mp4", "video/mp4")}
          blob={new Blob(["video"])}
          url="blob:video"
        />
      </>
    ));

    expect(container.querySelector('img[src="blob:image"]'))
      .toBeInTheDocument();
    expect(container.querySelector('iframe[src="blob:pdf"]'))
      .toBeInTheDocument();
    expect(container.querySelector('audio[src="blob:audio"]'))
      .toBeInTheDocument();
    expect(container.querySelector('video[src="blob:video"]'))
      .toBeInTheDocument();
  });

  it("shows a fallback when the browser cannot decode native media", () => {
    const { container } = render(() => (
      <AssetPreview
        reference={asset("recording.flac", "audio/flac")}
        blob={new Blob(["audio"])}
        url="blob:audio"
      />
    ));

    fireEvent.error(container.querySelector("audio")!);

    expect(screen.getByText("Your browser cannot play this audio."))
      .toBeInTheDocument();
    expect(container.querySelector("audio")).toBeNull();
  });

  it("renders bounded tabular data as text cells", async () => {
    render(() => (
      <AssetPreview
        reference={asset("data.csv", "text/csv")}
        blob={new Blob(["Name,Count\nApple,2"])}
        url="blob:csv"
      />
    ));

    expect(await screen.findByRole("columnheader", { name: "Name" }))
      .toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "Apple" })).toBeInTheDocument();
  });

  it("does not offer an active preview for SVG or HTML", () => {
    const { container } = render(() => (
      <>
        <AssetPreview
          reference={asset("diagram.svg", "image/svg+xml")}
          blob={new Blob(["<svg></svg>"])}
          url="blob:svg"
        />
        <AssetPreview
          reference={asset("page.html", "text/html")}
          blob={new Blob(["<script>alert(1)</script>"])}
          url="blob:html"
        />
      </>
    ));

    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("iframe")).toBeNull();
    expect(container.querySelector("script")).toBeNull();
    expect(screen.getAllByText("Preview is unavailable for this file type."))
      .toHaveLength(2);
  });
});
