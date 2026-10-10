// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import type { PageCapture } from "./content.js";
import { inspectCaptureArtwork } from "./structured-artwork.js";

function capture(): PageCapture {
  return {
    metadata: {
      kind: "article",
      url: "https://example.com/book",
      canonical_url: "https://example.com/book",
      title: "Book",
      saved_at: "2026-10-10T12:00:00Z",
      product_html:
        '<script type="application/ld+json">{"@type":"Book","image":"/cover.jpg"}</script>',
    },
    markdown: "A book",
    images: [],
    unresolved: ["https://example.com/social.jpg"],
    preview_candidates: ["https://example.com/social.jpg"],
  };
}

afterEach(() => vi.useRealTimers());

describe("structured artwork inspection", () => {
  it("sends only raw evidence and prioritizes Rust's exact artwork for subsequent capture", async () => {
    const page = capture();
    const inspect = vi.fn(async () => ({
      protocol_version: 4 as const,
      ok: true as const,
      preview_candidates: ["https://example.com/cover.jpg"],
    }));
    await inspectCaptureArtwork(page, inspect);
    expect(inspect).toHaveBeenCalledWith({
      protocol_version: 4,
      action: "inspect_page_metadata",
      url: page.metadata.url,
      product_html: page.metadata.product_html,
    });
    expect(page.preview_candidates).toEqual([
      "https://example.com/cover.jpg",
      "https://example.com/social.jpg",
    ]);
    expect(page.unresolved).toEqual([
      "https://example.com/cover.jpg",
      "https://example.com/social.jpg",
    ]);
    expect(page.metadata.kind).toBe("article");
  });

  it("does not refetch artwork bytes already captured, deduplicates, and caps response candidates", async () => {
    const page = capture();
    page.images = [
      { url: "https://example.com/cover.jpg", content_type: "image/png", data_base64: "AQID" },
    ];
    await inspectCaptureArtwork(page, async () => ({
      protocol_version: 4,
      ok: true,
      preview_candidates: [
        "https://example.com/cover.jpg",
        "https://example.com/cover.jpg",
        "https://example.com/second.jpg",
        "https://example.com/third.jpg",
        "https://example.com/too-many.jpg",
      ],
    }));
    expect(page.preview_candidates).toEqual([
      "https://example.com/cover.jpg",
      "https://example.com/second.jpg",
      "https://example.com/third.jpg",
      "https://example.com/social.jpg",
    ]);
    expect(page.unresolved).not.toContain("https://example.com/cover.jpg");
    expect(page.unresolved).not.toContain("https://example.com/too-many.jpg");
  });

  it("retains normal capture when an older host rejects inspection or transport fails", async () => {
    for (const inspect of [
      async () => ({
        protocol_version: 4 as const,
        ok: false as const,
        error: "invalid_request",
        message: "unknown action",
      }),
      async () => {
        throw new Error("Host unavailable");
      },
    ]) {
      const page = capture();
      const prior = structuredClone(page);
      await inspectCaptureArtwork(page, inspect);
      expect(page).toEqual(prior);
    }
  });

  it("fails optional inspection within two seconds if the host never answers", async () => {
    vi.useFakeTimers();
    const page = capture();
    const prior = structuredClone(page);
    const pending = inspectCaptureArtwork(page, () => new Promise(() => undefined));
    await vi.advanceTimersByTimeAsync(2_000);
    await pending;
    expect(page).toEqual(prior);
  });

  it("rejects malformed and non-HTTP artwork responses", async () => {
    const page = capture();
    await inspectCaptureArtwork(page, async () => ({
      protocol_version: 4,
      ok: true,
      preview_candidates: [
        "file:///tmp/cover.jpg",
        "https://user:secret@example.com/cover.jpg",
        "not a URL",
      ],
    }));
    expect(page.preview_candidates).toEqual(["https://example.com/social.jpg"]);
  });

  it("skips standalone captures, absent evidence, and UTF-8 evidence exceeding the bound", async () => {
    const inspect = vi.fn();
    for (const kind of ["image", "video", "quote"] as const) {
      const page = capture();
      page.metadata.kind = kind;
      await inspectCaptureArtwork(page, inspect);
    }
    const absent = capture();
    delete absent.metadata.product_html;
    await inspectCaptureArtwork(absent, inspect);
    const oversized = capture();
    oversized.metadata.product_html = "💡".repeat(131_073);
    await inspectCaptureArtwork(oversized, inspect);
    expect(inspect).not.toHaveBeenCalled();
  });
});
