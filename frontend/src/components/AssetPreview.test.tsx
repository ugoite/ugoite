import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";

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
    expect(container.querySelector('object[data="blob:pdf"]'))
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

  it("renders PDFs through a titled object, never an image element", () => {
    const { container } = render(() => (
      <AssetPreview
        reference={asset("report.pdf", "")}
        blob={new Blob(["pdf"])}
        url="blob:pdf-empty-media"
      />
    ));

    const frame = container.querySelector(
      'object[data="blob:pdf-empty-media"]',
    );
    expect(frame).toBeInTheDocument();
    expect(frame).toHaveAttribute("title", "report.pdf");
    expect(frame).toHaveAttribute("type", "application/pdf");
    expect(container.querySelector("img")).toBeNull();
  });

  it("keeps the PDF fallback in the DOM even on success", () => {
    const onDownload = vi.fn();
    const { container } = render(() => (
      <AssetPreview
        reference={asset("report.pdf", "application/pdf")}
        blob={new Blob(["pdf"])}
        url="blob:pdf-ok"
        onDownload={onDownload}
      />
    ));

    const frame = container.querySelector('object[data="blob:pdf-ok"]');
    expect(frame).toBeInTheDocument();
    // Fallback children stay in the DOM so the file stays usable.
    expect(container.querySelector(".ui-asset-pdf-fallback"))
      .toBeInTheDocument();
    expect(screen.getByText("report.pdf")).toBeInTheDocument();
    expect(screen.getByText(/byte/)).toBeInTheDocument();
    const download = screen.getByRole("button", { name: "Download" });
    fireEvent.click(download);
    expect(onDownload).toHaveBeenCalledTimes(1);
  });

  it("offers a direct download link inside the object fallback without a handler", () => {
    const { container } = render(() => (
      <AssetPreview
        reference={asset("report.pdf", "application/pdf")}
        blob={new Blob(["pdf"])}
        url="blob:pdf-no-handler"
      />
    ));

    const frame = container.querySelector(
      'object[data="blob:pdf-no-handler"]',
    );
    expect(frame).toBeInTheDocument();
    const link = container.querySelector('a[download="report.pdf"]');
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute("href", "blob:pdf-no-handler");
  });
});
