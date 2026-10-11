// SPDX-License-Identifier: MIT

import { describe, expect, it } from "vitest";

import { extractPageMetadata } from "./page-metadata.js";

function buildDoc(head: string, title = "Document title"): Document {
  const doc = document.implementation.createHTMLDocument(title);
  doc.head.insertAdjacentHTML("beforeend", head);
  return doc;
}

describe("extractPageMetadata", () => {
  it("collects canonical and social metadata in deterministic priority order", () => {
    const doc = buildDoc(`
      <link rel="canonical" href="/canonical">
      <meta property="og:url" content="https://ignored.example.com/open-graph">
      <meta property="og:title" content="Open Graph title">
      <meta name="twitter:title" content="Twitter title">
      <meta property="og:site_name" content="Example Journal">
      <meta property="og:description" content="Open Graph description">
      <meta name="description" content="Plain description">
      <meta name="theme-color" content="#123456">
      <meta property="og:image:secure_url" content="/secure-social.png">
      <meta property="og:image" content="https://cdn.example.com/social.jpg">
      <meta name="twitter:image" content="/twitter.png">
    `);

    const result = extractPageMetadata(doc, "https://example.com/post?ref=feed");

    expect(result).toMatchObject({
      canonicalUrl: "https://example.com/canonical",
      title: "Open Graph title",
      site: "Example Journal",
      excerpt: "Open Graph description",
      themeColor: "#123456",
    });
    expect(result.socialImageUrls).toEqual([
      "https://example.com/secure-social.png",
      "https://cdn.example.com/social.jpg",
      "https://example.com/twitter.png",
    ]);
  });

  it("resolves iframe canonical and artwork metadata against its document base", () => {
    const doc = buildDoc(`
      <base href="https://frame.example/embedded/">
      <link rel="canonical" href="../canonical">
      <meta property="og:image" content="cover.jpg">
      <link rel="icon" href="/favicon.png">
    `);

    const result = extractPageMetadata(doc, "https://top.example/gallery");

    expect(result.canonicalUrl).toBe("https://frame.example/canonical");
    expect(result.socialImageUrls).toEqual(["https://frame.example/embedded/cover.jpg"]);
    expect(result.faviconUrls).toEqual(["https://frame.example/favicon.png"]);
  });

  it("uses the owning live document URL when there is no base element", () => {
    const previousUrl = window.location.href;
    const previousHead = document.head.innerHTML;
    try {
      window.location.href = "https://frame.example/embedded/page";
      document.head.innerHTML = '<meta property="og:image" content="cover.jpg">';

      const result = extractPageMetadata(document, "https://top.example/gallery");

      expect(result.canonicalUrl).toBe("https://top.example/gallery");
      expect(result.socialImageUrls).toEqual(["https://frame.example/embedded/cover.jpg"]);
    } finally {
      document.head.innerHTML = previousHead;
      window.location.href = previousUrl;
    }
  });

  it("keeps the supplied page origin when an iframe has no canonical metadata", () => {
    const doc = buildDoc('<base href="https://frame.example/embedded/">');

    expect(extractPageMetadata(doc, "https://top.example/gallery").canonicalUrl).toBe(
      "https://top.example/gallery",
    );
  });

  it("prefers the largest declared raster favicon and keeps fallbacks", () => {
    const doc = buildDoc(`
      <link rel="shortcut icon" href="/small.ico" sizes="16x16">
      <link rel="icon" href="/large.png" type="image/png" sizes="64x64">
      <link rel="apple-touch-icon" href="/touch.png" sizes="180x180">
      <link rel="icon" href="/unsafe.svg" type="image/svg+xml" sizes="any">
      <link rel="icon" href="data:image/png;base64,AAAA" sizes="256x256">
    `);

    expect(extractPageMetadata(doc, "https://example.com/post").faviconUrls).toEqual([
      "https://example.com/touch.png",
      "https://example.com/large.png",
      "https://example.com/small.ico",
    ]);
  });

  it("uses the first non-empty standard or underscore theme colour", () => {
    const doc = buildDoc(`
      <meta name="theme-color" content="   ">
      <meta name="theme_color" content=" rgb(12, 34, 56) ">
      <meta name="theme-color" content="#ignored">
    `);

    expect(extractPageMetadata(doc, "https://example.com/post").themeColor).toBe("rgb(12, 34, 56)");
  });

  it("rejects non-http and credential-bearing image metadata", () => {
    const doc = buildDoc(`
      <meta property="og:image" content="file:///tmp/private.png">
      <meta name="twitter:image" content="https://user:secret@example.com/private.png">
      <link rel="icon" href="javascript:alert(1)">
    `);

    const result = extractPageMetadata(doc, "https://example.com/post");
    expect(result.socialImageUrls).toEqual([]);
    expect(result.faviconUrls).toEqual([]);
    expect(result.themeColor).toBeUndefined();
  });
});
