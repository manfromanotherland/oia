// SPDX-License-Identifier: MIT

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PageCapture } from "./content.js";
import { buildSaveSelectionRequest } from "./selection-request.js";

const imageMock = vi.hoisted(() => ({ fetchImages: vi.fn() }));
vi.mock("./log.js", () => ({ log: async () => undefined }));
vi.mock("./images.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./images.js")>();
  return {
    ...actual,
    fetchImages: imageMock.fetchImages,
    // Exercise the real capping policy with tiny assets instead of allocating 40 MiB.
    capTotalBytes: (images: Parameters<typeof actual.capTotalBytes>[0]) =>
      actual.capTotalBytes(images, 6),
  };
});

function page(): PageCapture {
  return {
    metadata: {
      kind: "article",
      url: "https://example.com/book",
      canonical_url: "https://example.com/book",
      title: "Book page",
      saved_at: "2026-10-10T12:00:00Z",
      product_html:
        '<script type="application/ld+json">{"@type":"Book","image":"/cover.png"}</script>',
    },
    markdown: "Full article content.",
    images: [
      { url: "https://example.com/social.png", content_type: "image/png", data_base64: "AQID" },
      { url: "https://example.com/content.png", content_type: "image/png", data_base64: "BAUG" },
    ],
    unresolved: [],
    preview_candidates: ["https://example.com/social.png"],
    content_image_urls: ["https://example.com/content.png"],
  };
}

beforeEach(() => imageMock.fetchImages.mockReset());
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function background() {
  vi.resetModules();
  const event = () => ({ addListener: vi.fn() });
  const sendNativeMessage = vi.fn(
    (_host: string, request: { action: string }, callback: (response: object) => void) => {
      expect(request.action).toBe("inspect_page_metadata");
      callback({
        protocol_version: 4,
        ok: true,
        preview_candidates: ["https://example.com/cover.png"],
      });
    },
  );
  vi.stubGlobal("chrome", {
    action: { setIcon: vi.fn(), setBadgeText: vi.fn(), setBadgeBackgroundColor: vi.fn() },
    commands: { onCommand: event() },
    contextMenus: { onClicked: event(), removeAll: vi.fn(), create: vi.fn() },
    notifications: { onClicked: event() },
    runtime: {
      onInstalled: event(),
      onStartup: event(),
      onConnect: event(),
      onMessage: event(),
      sendNativeMessage,
      lastError: undefined,
    },
    tabs: { onActivated: event(), onUpdated: event() },
  });
  return { ...(await import("./background.js")), sendNativeMessage };
}

describe("background cultural artwork capture", () => {
  it("captures the exact structured cover and gives it priority over social and content images", async () => {
    const bg = await background();
    const capture = page();
    imageMock.fetchImages.mockResolvedValue({
      images: [
        { url: "https://example.com/cover.png", content_type: "image/png", data_base64: "BwgJ" },
      ],
      unresolved: [],
    });
    const images = await bg.completeCaptureImages(capture);
    expect(bg.sendNativeMessage).toHaveBeenCalledTimes(1);
    expect(imageMock.fetchImages).toHaveBeenCalledWith(["https://example.com/cover.png"]);
    expect(images.map((image) => image.url)).toEqual([
      "https://example.com/cover.png",
      "https://example.com/content.png",
    ]);
    expect(capture.preview_url).toBe("https://example.com/cover.png");
  });

  it("retains social preview bytes when the selected cover cannot be captured", async () => {
    const bg = await background();
    const capture = page();
    imageMock.fetchImages.mockResolvedValue({
      images: [],
      unresolved: ["https://example.com/cover.png"],
    });
    const images = await bg.completeCaptureImages(capture);
    expect(images.map((image) => image.url)).toEqual([
      "https://example.com/social.png",
      "https://example.com/content.png",
    ]);
    expect(capture.preview_url).toBe("https://example.com/social.png");
  });

  it("preserves selection context and save_selection routing after cover capture", async () => {
    const bg = await background();
    const capture = {
      ...page(),
      selected_text: "Selected ordinary passage.",
      selection_context: { version: 1 as const, selected_text: "Selected ordinary passage." },
    };
    imageMock.fetchImages.mockResolvedValue({
      images: [
        { url: "https://example.com/cover.png", content_type: "image/png", data_base64: "BwgJ" },
      ],
      unresolved: [],
    });
    capture.images = await bg.completeCaptureImages(capture);
    const request = buildSaveSelectionRequest(capture);
    expect(request.action).toBe("save_selection");
    expect(request.selection_context).toEqual(capture.selection_context);
    expect(request.preview_url).toBe("https://example.com/cover.png");
    expect(request.images[0].url).toBe("https://example.com/cover.png");
    expect(request.metadata.product_html).toBe(capture.metadata.product_html);
  });
});
