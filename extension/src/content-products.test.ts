// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import type { PageCapture } from "./content.js";
import { buildSaveLinkRequest } from "./toolbar.js";

type RuntimeMessageListener = (
  message: unknown,
  sender: chrome.runtime.MessageSender,
  sendResponse: (response: PageCapture) => void,
) => boolean | void;

afterEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
});

describe("live product page capture", () => {
  it.each(["extract", "capture-link"])("passes structured evidence through %s", async (action) => {
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
    document.head.innerHTML = `<title>Table lamp</title><script type="application/ld+json">{"@type":"Product","offers":{"price":"229.00","priceCurrency":"EUR"}}</script>`;
    document.body.innerHTML = `<article><p>${"A table lamp made for thoughtful reading and evening light. ".repeat(40)}</p></article>`;
    await import("./content.js");

    const capture = await new Promise<PageCapture>((resolve) => {
      expect(listener!({ action, pageUrl: "https://example.com/lamp" }, {}, resolve)).toBe(true);
    });

    expect(capture.metadata.kind).toBe("article");
    expect(capture.metadata.product_html).toContain('"price":"229.00"');
    expect(capture.markdown).not.toContain("priceCurrency");
    if (action === "capture-link") {
      const request = buildSaveLinkRequest(capture);
      expect(request.metadata.product_html).toBe(capture.metadata.product_html);
      expect(request.action).toBe("save_link");
      expect(request).not.toHaveProperty("markdown");
    } else {
      expect(capture.markdown).toContain("A table lamp");
    }
  });
});
