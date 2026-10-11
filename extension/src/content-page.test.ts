// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import type { PageCapture } from "./content.js";

vi.mock("./images.js", () => ({
  fetchImages: vi.fn(async (urls: string[]) => ({
    images: urls.map((url) => ({ url, content_type: "image/png", data_base64: "AQID" })),
    unresolved: [],
  })),
}));

type CaptureResponse = PageCapture | { error: string };
type RuntimeMessageListener = (
  message: unknown,
  sender: chrome.runtime.MessageSender,
  sendResponse: (response: CaptureResponse) => void,
) => boolean | void;

afterEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
});

async function capture(pageUrl: string, action = "capture-page"): Promise<CaptureResponse> {
  vi.resetModules();
  let listener: RuntimeMessageListener | undefined;
  vi.stubGlobal("chrome", {
    runtime: {
      onMessage: {
        addListener: (callback: RuntimeMessageListener) => {
          listener = callback;
        },
      },
    },
  });
  await import("./content.js");
  return new Promise<CaptureResponse>((resolve) => {
    expect(listener!({ action, pageUrl }, {}, resolve)).toBe(true);
  });
}

describe("neutral page capture", () => {
  it("captures article prose and raw content measurements for the requested page URL", async () => {
    document.head.innerHTML = "<title>An article</title>";
    const paragraph = "A detailed account of saving useful ideas for later reading. ".repeat(12);
    document.body.innerHTML = `<article><p>${paragraph}<a href='/related'>Related reading</a></p><p>${paragraph}</p></article>`;

    const result = await capture("https://example.com/article");

    expect(result).not.toHaveProperty("error");
    const page = result as PageCapture;
    expect(page.metadata.url).toBe("https://example.com/article");
    expect(page.markdown).toContain("A detailed account");
    expect(page.page_content?.text_characters).toBeGreaterThan(500);
    expect(page.page_content?.paragraph_characters).toHaveLength(2);
    expect(page.page_content?.link_characters).toBe("Related reading".length);
    expect(page.metadata).not.toHaveProperty("link_category");
  });

  it("returns homepage metadata and local preview roles when no content is extracted", async () => {
    document.head.innerHTML = [
      "<title>Sesame</title>",
      '<meta name="description" content="A voice companion">',
      '<meta property="og:image" content="https://example.com/preview.png">',
      '<link rel="icon" href="https://example.com/favicon.png">',
    ].join("");
    document.body.innerHTML = "<main><canvas></canvas></main>";

    const result = await capture("https://example.com/");

    expect(result).not.toHaveProperty("error");
    const page = result as PageCapture;
    expect(page.metadata).toMatchObject({
      url: "https://example.com/",
      title: "Sesame",
      excerpt: "A voice companion",
    });
    expect(page.markdown).toBe("");
    expect(page.page_content).toBeUndefined();
    expect(page.preview_candidates).toEqual(["https://example.com/preview.png"]);
    expect(page.favicon_candidates).toEqual(["https://example.com/favicon.png"]);
    expect(page.images.map((image) => image.url)).toEqual([
      "https://example.com/preview.png",
      "https://example.com/favicon.png",
    ]);
  });

  it.each(["Movie", "Product"])(
    "preserves raw %s evidence on a metadata-only page without classifying it",
    async (entityType) => {
      document.head.innerHTML = `<title>A catalog entry</title><script type="application/ld+json">{"@type":"${entityType}","name":"A catalog entry","image":"https://example.com/cover.png"}</script>`;
      document.body.innerHTML = "";

      const result = await capture("https://example.com/catalog/entry");

      expect(result).not.toHaveProperty("error");
      const page = result as PageCapture;
      expect(page.markdown).toBe("");
      expect(page.metadata.product_html).toContain(`"@type":"${entityType}"`);
      expect(page.metadata).not.toHaveProperty("link_category");
      expect(page.page_content).toBeUndefined();
    },
  );

  it("retains the legacy extract error when the document has no readable content", async () => {
    document.head.innerHTML = "<title>No article</title>";
    document.body.innerHTML = "";

    await expect(capture("https://example.com/", "extract")).resolves.toEqual({
      error: "Could not extract article content from this page.",
    });
  });
});
