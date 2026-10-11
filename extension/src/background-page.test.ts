// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import type { PageCapture } from "./content.js";
import type { SavePageRequest } from "./protocol.js";

vi.mock("./log.js", () => ({ log: async () => undefined }));
vi.mock("./images.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./images.js")>()),
  fetchImages: async () => ({ images: [], unresolved: [] }),
}));

type RuntimeListener = (
  message: unknown,
  sender: chrome.runtime.MessageSender,
  respond: (response: unknown) => void,
) => boolean | void;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function capturedPage(): PageCapture {
  return {
    metadata: {
      kind: "article",
      url: "https://example.com/page",
      canonical_url: "https://example.com/page",
      title: "Page title",
      saved_at: "2026-10-11T12:00:00Z",
    },
    markdown: "Captured page content.",
    page_content: { text_characters: 900, link_characters: 20, paragraph_characters: [450, 430] },
    images: [],
    unresolved: [],
  };
}

async function harness(
  capture?: PageCapture | { error: string } | null,
  url = "https://example.com/page",
  unreachable = false,
) {
  vi.resetModules();
  let runtimeListener: RuntimeListener | undefined;
  const event = () => ({ addListener: vi.fn() });
  const tab = { id: 42, url, title: "Tab title" };
  const requests: SavePageRequest[] = [];
  const sendMessage = vi.fn(async (_tabId: number, message: { action: string }) => {
    if (message.action !== "capture-page") return undefined;
    if (unreachable) throw new Error("The content script could not be reached.");
    return capture;
  });
  vi.stubGlobal("chrome", {
    action: { setIcon: vi.fn(), setBadgeText: vi.fn(), setBadgeBackgroundColor: vi.fn() },
    commands: { onCommand: event() },
    contextMenus: { onClicked: event(), removeAll: vi.fn(), create: vi.fn() },
    notifications: { onClicked: event(), create: vi.fn(), clear: vi.fn() },
    runtime: {
      onInstalled: event(),
      onStartup: event(),
      onConnect: event(),
      onMessage: {
        addListener: (listener: RuntimeListener) => {
          runtimeListener = listener;
        },
      },
      lastError: undefined,
      sendNativeMessage: (
        _host: string,
        request: SavePageRequest | { action: "inspect_page_metadata" },
        callback: (response: object) => void,
      ) => {
        if (request.action === "inspect_page_metadata") {
          callback({ protocol_version: 4, ok: true, preview_candidates: [] });
          return;
        }
        requests.push(request);
        callback({ protocol_version: 4, ok: true, id: "page-1", path: "article.md" });
      },
    },
    scripting: { executeScript: vi.fn(async () => [{ result: false }]) },
    tabs: { onActivated: event(), onUpdated: event(), get: async () => tab, sendMessage },
  });
  await import("./background.js");
  await new Promise((resolve) => {
    runtimeListener!({ action: "toolbar-save", kind: "page", tabId: 42 }, {}, resolve);
  });
  return { requests, sendMessage };
}

describe("automatic page save", () => {
  it("passes extracted content and raw prose evidence to the engine in one save_page request", async () => {
    const capture = capturedPage();
    const result = await harness(capture);
    expect(result.requests).toHaveLength(1);
    expect(result.requests[0]).toMatchObject({
      action: "save_page",
      metadata: capture.metadata,
      markdown: capture.markdown,
      page_content: capture.page_content,
    });
  });

  it("passes a metadata-only movie page to the engine without choosing an article or bookmark", async () => {
    const capture = capturedPage();
    capture.markdown = "";
    delete capture.page_content;
    capture.metadata.product_html =
      '<script type="application/ld+json">{"@type":"Movie","name":"Movie title"}</script>';
    const result = await harness(capture);
    expect(result.requests).toHaveLength(1);
    expect(result.requests[0]).toMatchObject({
      action: "save_page",
      markdown: "",
      metadata: { product_html: capture.metadata.product_html },
    });
    expect(result.requests[0]).not.toHaveProperty("page_content");
  });

  it.each([undefined, null, { error: "Page extraction failed." }])(
    "falls back to tab metadata when page capture returns %s",
    async (capture) => {
      const result = await harness(capture);
      expect(result.requests).toHaveLength(1);
      expect(result.requests[0]).toMatchObject({
        action: "save_page",
        metadata: { url: "https://example.com/page", title: "Tab title" },
        markdown: "",
      });
      expect(result.requests[0]).not.toHaveProperty("page_content");
    },
  );

  it("falls back when the content script remains unreachable after injection", async () => {
    const result = await harness(undefined, "https://example.com/page", true);
    expect(result.requests).toHaveLength(1);
    expect(result.requests[0]).toMatchObject({
      action: "save_page",
      metadata: { url: "https://example.com/page", title: "Tab title" },
      markdown: "",
    });
  });

  it("rejects a browser-internal URL instead of saving a malformed fallback", async () => {
    const result = await harness(undefined, "chrome://extensions");
    expect(result.requests).toHaveLength(0);
    expect(result.sendMessage).toHaveBeenCalledWith(
      42,
      expect.objectContaining({ status: "error", title: "Couldn't save page" }),
      { frameId: 0 },
    );
  });
});
