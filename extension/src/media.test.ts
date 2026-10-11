// SPDX-License-Identifier: MIT

import { describe, expect, it } from "vitest";

import { extractQuote, extractStandaloneMedia } from "./media.js";

function buildDoc(head: string, body: string, title = "Page title"): Document {
  const doc = document.implementation.createHTMLDocument(title);
  doc.title = title;
  doc.head.insertAdjacentHTML("beforeend", head);
  doc.body.innerHTML = body;
  return doc;
}

describe("extractStandaloneMedia", () => {
  it("omits product evidence from standalone images, videos, and selected quotes", () => {
    const doc = buildDoc(
      '<script type="application/ld+json">{"@type":"Product","offers":{"price":"229.00","priceCurrency":"EUR"}}</script>',
      '<img src="https://example.com/lamp.png"><video src="https://example.com/demo.mp4"></video>',
    );
    const image = extractStandaloneMedia(
      doc,
      "https://example.com/lamp",
      "image",
      "https://example.com/lamp.png",
    );
    const video = extractStandaloneMedia(
      doc,
      "https://example.com/lamp",
      "video",
      "https://example.com/demo.mp4",
    );
    const quote = extractQuote(doc, "https://example.com/lamp", "A selected thought.");
    for (const result of [image, video, quote]) {
      expect(result.metadata).not.toHaveProperty("product_html");
    }
  });

  it("builds a standalone image item with page metadata and capture bytes requested", () => {
    const imageUrl = "https://cdn.example.com/art/diagram.png";
    const doc = buildDoc(
      [
        '<link rel="canonical" href="https://example.com/canonical">',
        '<meta property="og:site_name" content="Example Journal">',
        '<meta name="author" content="Jane Doe">',
        '<meta name="theme-color" content="#123456">',
      ].join(""),
      `<figure><img src="${imageUrl}" alt="System diagram"><figcaption>The whole system</figcaption></figure>`,
      "Architecture article",
    );

    const result = extractStandaloneMedia(
      doc,
      "https://example.com/post?ref=feed",
      "image",
      imageUrl,
      "2026-08-23T10:00:00.000Z",
    );

    expect(result.metadata).toMatchObject({
      kind: "image",
      url: "https://example.com/post?ref=feed",
      canonical_url: "https://example.com/canonical",
      media_url: imageUrl,
      title: "Architecture article",
      author: "Jane Doe",
      site: "Example Journal",
      theme_color: "#123456",
      saved_at: "2026-08-23T10:00:00.000Z",
    });
    expect(result.markdown).toBe(`![System diagram](${imageUrl})`);
    expect(result.image_urls).toEqual([imageUrl]);
  });

  it("reads iframe image attributes against their document while keeping the top-level origin", () => {
    const imageUrl = "https://frame.example/embedded/art/image.png";
    const doc = buildDoc(
      '<base href="https://frame.example/embedded/"><link rel="canonical" href="../canonical">',
      '<img src="art/image.png" alt="Frame artwork">',
      "Embedded gallery",
    );

    const result = extractStandaloneMedia(doc, "https://top.example/gallery", "image", imageUrl);

    expect(result.metadata).toMatchObject({
      url: "https://top.example/gallery",
      canonical_url: "https://frame.example/canonical",
      media_url: imageUrl,
    });
    expect(result.markdown).toBe(`![Frame artwork](${imageUrl})`);
    expect(result.image_urls).toEqual([imageUrl]);
  });

  it("resolves an iframe video's relative durable source and poster in its owning document", () => {
    const dataUrl = "data:video/mp4;base64,LOCAL_VIDEO";
    const doc = buildDoc(
      '<base href="https://frame.example/embedded/"><link rel="canonical" href="/clip">',
      `<video src="${dataUrl}" poster="poster.jpg" aria-label="Embedded clip"><source src="../media/clip.mp4"></video>`,
    );

    const result = extractStandaloneMedia(doc, "https://top.example/gallery", "video", dataUrl);

    expect(result.metadata).toMatchObject({
      url: "https://top.example/gallery",
      canonical_url: "https://frame.example/clip",
      media_url: "https://frame.example/media/clip.mp4",
    });
    expect(result.markdown).toBe(
      "![Embedded clip](https://frame.example/embedded/poster.jpg)\n\n[Watch video](https://frame.example/media/clip.mp4)",
    );
    expect(result.image_urls).toEqual(["https://frame.example/embedded/poster.jpg"]);
  });

  it("stores a video URL as metadata/link and captures only its poster", () => {
    const videoUrl = "https://media.example.com/demo.mp4";
    const doc = buildDoc(
      "",
      `<video poster="/images/demo-poster.jpg" aria-label="Product demo"><source src="https://media.example.com/fallback.webm"><source src="${videoUrl}"></video>`,
      "Launch page",
    );

    const result = extractStandaloneMedia(doc, "https://example.com/launch", "video", videoUrl);

    expect(result.metadata.kind).toBe("video");
    expect(result.metadata.url).toBe("https://example.com/launch");
    expect(result.metadata.media_url).toBe(videoUrl);
    expect(result.metadata.title).toBe("Launch page");
    expect(result.metadata.site).toBe("example.com");
    expect(result.markdown).toBe(
      `![Product demo](https://example.com/images/demo-poster.jpg)\n\n[Watch video](${videoUrl})`,
    );
    expect(result.image_urls).toEqual(["https://example.com/images/demo-poster.jpg"]);
    expect(result.image_urls).not.toContain(videoUrl);
  });

  it("uses the page social image as a video poster when the element has none", () => {
    const videoUrl = "https://media.example.com/demo.m3u8";
    const doc = buildDoc(
      '<meta property="og:image" content="https://cdn.example.com/social-card.jpg">',
      `<video src="${videoUrl}"></video>`,
      "Launch keynote",
    );

    const result = extractStandaloneMedia(doc, "https://example.com/keynote", "video", videoUrl);

    expect(result.image_urls).toEqual(["https://cdn.example.com/social-card.jpg"]);
    expect(result.markdown).toContain("![Launch keynote](https://cdn.example.com/social-card.jpg)");
    expect(result.markdown).toContain(`[Watch video](${videoUrl})`);
  });

  it("never asks the image fetcher for video bytes when no poster exists", () => {
    const videoUrl = "https://media.example.com/clip.webm";
    const result = extractStandaloneMedia(
      buildDoc("", `<video src="${videoUrl}"></video>`, "Clip"),
      "https://example.com/watch",
      "video",
      videoUrl,
    );

    expect(result.image_urls).toEqual([]);
    expect(result.markdown).toBe(`[Watch video](${videoUrl})`);
  });

  it("prefers a durable source when the clicked video URL is a data payload", () => {
    const dataUrl = "data:video/mp4;base64,DO_NOT_PERSIST_THIS_PAYLOAD";
    const doc = buildDoc(
      '<link rel="canonical" href="https://example.com/canonical-watch">',
      `<video src="${dataUrl}"><source src="/media/durable.mp4"></video>`,
      "Durable fallback",
    );

    const result = extractStandaloneMedia(
      doc,
      "https://example.com/watch?session=one",
      "video",
      dataUrl,
    );
    const persisted = JSON.stringify({ metadata: result.metadata, markdown: result.markdown });

    expect(result.metadata.media_url).toBe("https://example.com/media/durable.mp4");
    expect(result.markdown).toBe("[Watch video](https://example.com/media/durable.mp4)");
    expect(persisted).not.toContain("data:");
    expect(persisted).not.toContain("DO_NOT_PERSIST_THIS_PAYLOAD");
  });

  it("keeps distinct legacy blob identities for callers outside the production stream path", () => {
    const firstBlob = "blob:https://example.com/session-video-one";
    const secondBlob = "blob:https://example.com/session-video-two";
    const canonicalUrl = "https://example.com/canonical-watch";
    const pageUrl = "https://example.com/watch?session=one";
    const doc = buildDoc(
      `<link rel="canonical" href="${canonicalUrl}">`,
      `<video src="${firstBlob}"></video><video src="${secondBlob}"></video>`,
      "Two embedded videos",
    );

    const first = extractStandaloneMedia(doc, pageUrl, "video", firstBlob);
    const second = extractStandaloneMedia(doc, pageUrl, "video", secondBlob);
    const persisted = JSON.stringify([
      first.metadata,
      first.markdown,
      second.metadata,
      second.markdown,
    ]);

    expect(first.metadata).toMatchObject({
      url: pageUrl,
      canonical_url: canonicalUrl,
      media_url: `oia-video:${encodeURIComponent(canonicalUrl)}:1`,
    });
    expect(second.metadata.media_url).toBe(`oia-video:${encodeURIComponent(canonicalUrl)}:2`);
    expect(first.metadata.media_url).not.toBe(second.metadata.media_url);
    expect(first.markdown).toBe(`[Watch video](${pageUrl})`);
    expect(second.markdown).toBe(`[Watch video](${pageUrl})`);
    expect(persisted).not.toContain("blob:");
    expect(persisted).not.toContain("session-video-one");
    expect(persisted).not.toContain("session-video-two");
  });

  it("falls back to a readable filename when the page has no useful title", () => {
    const imageUrl = "https://cdn.example.com/photos/summer-sunset.webp";
    const result = extractStandaloneMedia(
      buildDoc("", `<img src="${imageUrl}">`, ""),
      "https://example.com/gallery",
      "image",
      imageUrl,
    );

    expect(result.metadata.title).toBe("summer sunset");
    expect(result.markdown).toBe(`![summer sunset](${imageUrl})`);
  });

  it("builds a paragraph-safe quote with origin metadata and card preview fields", () => {
    const doc = buildDoc(
      [
        '<link rel="canonical" href="https://example.com/canonical-essay">',
        '<meta property="og:site_name" content="Example Journal">',
        '<meta name="theme_color" content="#654321">',
      ].join(""),
      "<article>Source body</article>",
      "Source essay",
    );

    const result = extractQuote(
      doc,
      "https://example.com/essay?from=home",
      " First line\r\ncontinues\r\n\r\nSecond paragraph\r\nlast line ",
      "2026-08-23T11:00:00.000Z",
    );

    expect(result.metadata).toMatchObject({
      kind: "quote",
      url: "https://example.com/essay?from=home",
      canonical_url: "https://example.com/canonical-essay",
      title: "Source essay",
      site: "Example Journal",
      theme_color: "#654321",
      saved_at: "2026-08-23T11:00:00.000Z",
      excerpt: "First line continues Second paragraph last line",
      word_count: 7,
    });
    expect(result.metadata.media_url).toBeUndefined();
    expect(result.markdown).toBe("> First line\n> continues\n>\n> Second paragraph\n> last line");
    expect(result.image_urls).toEqual([]);
  });

  it("bounds a quote excerpt by Unicode characters without truncating its Markdown", () => {
    const selectedText = "🙂".repeat(610);
    const result = extractQuote(
      buildDoc("", "", "Emoji essay"),
      "https://example.com/emoji",
      selectedText,
    );

    expect(result.metadata.excerpt).toBe(`${"🙂".repeat(599)}…`);
    expect(Array.from(result.metadata.excerpt!)).toHaveLength(600);
    expect(result.markdown).toBe(`> ${selectedText}`);
  });
});
