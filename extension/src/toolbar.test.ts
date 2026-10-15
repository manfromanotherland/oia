// SPDX-License-Identifier: MIT

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import type { PageCapture } from "./content.js";
import {
  buildFallbackLinkCapture,
  buildSaveLinkRequest,
  buildScreenshotCapture,
  isStableScreenshotDocument,
  isToolbarSaveKind,
  isToolbarSaveMessage,
  toolbarSaveMessage,
} from "./toolbar.js";

describe("toolbar save", () => {
  it("lets toolbar clicks reach the save handler without opening a browser popup", () => {
    const manifest = JSON.parse(readFileSync(resolve(process.cwd(), "manifest.json"), "utf8"));

    expect(manifest.action).toMatchObject({ default_title: "Save to Óia" });
    expect(manifest.action).not.toHaveProperty("default_popup");
  });

  it("builds and validates typed toolbar messages", () => {
    expect(toolbarSaveMessage("page", 42)).toEqual({
      action: "toolbar-save",
      kind: "page",
      tabId: 42,
    });
    expect(isToolbarSaveMessage({ action: "toolbar-save", kind: "page", tabId: 42 })).toBe(true);
    expect(isToolbarSaveMessage({ action: "toolbar-save", kind: "screenshot", tabId: 42 })).toBe(
      true,
    );
    expect(isToolbarSaveMessage({ action: "toolbar-save", kind: "video", tabId: 42 })).toBe(false);
    expect(isToolbarSaveMessage({ action: "toolbar-save", kind: "page" })).toBe(false);
  });

  it("rejects legacy article and link menu intents", () => {
    for (const kind of ["article", "link"]) {
      expect(isToolbarSaveKind(kind)).toBe(false);
      expect(isToolbarSaveMessage({ action: "toolbar-save", kind, tabId: 42 })).toBe(false);
    }
  });

  it("accepts only a stable document around each screenshot tile", () => {
    const before = { id: 42, url: "https://example.com/one", pendingUrl: undefined };
    expect(isStableScreenshotDocument(before, { ...before })).toBe(true);
    expect(isStableScreenshotDocument(before, { ...before, url: `${before.url}#section` })).toBe(
      true,
    );
    expect(isStableScreenshotDocument(before, { ...before, id: 43 })).toBe(false);
    expect(isStableScreenshotDocument(before, { ...before, url: "https://example.com/two" })).toBe(
      false,
    );
    expect(
      isStableScreenshotDocument(before, {
        ...before,
        pendingUrl: "https://example.com/two",
      }),
    ).toBe(false);
  });
});

describe("link save", () => {
  it("keeps captured page metadata and selected local asset roles", () => {
    const capture: PageCapture = {
      metadata: {
        kind: "article",
        url: "https://example.com/post",
        canonical_url: "https://example.com/canonical",
        title: "Social title",
        site: "Example",
        theme_color: "#123456",
        excerpt: "Social description",
        saved_at: "2026-08-25T09:00:00.000Z",
      },
      markdown: "",
      images: [
        { url: "https://example.com/social.png", content_type: "image/png", data_base64: "AQID" },
      ],
      unresolved: [],
      preview_url: "https://example.com/social.png",
      favicon_url: "https://example.com/favicon.ico",
    };

    expect(buildSaveLinkRequest(capture)).toEqual({
      protocol_version: 4,
      action: "save_link",
      metadata: capture.metadata,
      images: capture.images,
      preview_url: capture.preview_url,
      favicon_url: capture.favicon_url,
    });
  });

  it("falls back to tab metadata without pretending content was extracted", () => {
    const capture = buildFallbackLinkCapture(
      { url: "https://example.com/post", title: "  Page   title  " },
      "2026-08-25T09:00:00.000Z",
    );

    expect(capture.metadata).toMatchObject({
      kind: "article",
      url: "https://example.com/post",
      canonical_url: "https://example.com/post",
      title: "Page title",
      site: "example.com",
    });
    expect(capture.images).toEqual([]);
    expect(capture.markdown).toBe("");
    expect(capture.metadata.theme_color).toBeUndefined();
  });
});

describe("screenshot save", () => {
  it("uses a raw-byte SHA-256 local identity and never persists the data URL", async () => {
    const capture = await buildScreenshotCapture(
      { url: "https://example.com/post", title: "Example [page]" },
      "data:image/png;base64,AQIDBA==",
      "2026-08-25T09:00:00.000Z",
    );
    const identity =
      "cuttings-asset:assets/9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a.png";

    expect(capture.metadata).toMatchObject({
      kind: "image",
      media_url: identity,
      url: "https://example.com/post",
      title: "Example [page]",
    });
    expect(capture.markdown).toBe(`![Screenshot of Example \\[page\\]](${identity})`);
    expect(capture.images).toEqual([
      { url: identity, content_type: "image/png", data_base64: "AQIDBA==" },
    ]);
    expect(JSON.stringify(capture)).not.toContain("data:image/png");
  });

  it("rejects unsupported pages and malformed screenshot payloads", async () => {
    await expect(
      buildScreenshotCapture(
        { url: "chrome://extensions", title: "Extensions" },
        "data:image/png;base64,AQID",
      ),
    ).rejects.toThrow("Only HTTP(S) pages");
    await expect(
      buildScreenshotCapture(
        { url: "https://example.com", title: "Example" },
        "data:image/jpeg;base64,AQID",
      ),
    ).rejects.toThrow("PNG screenshot");
  });
});
