// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import type { CaptureFailure, PageCapture } from "./content.js";
import type { SavePageRequest } from "./protocol.js";

const budget = vi.hoisted(() => ({ maxBytes: undefined as number | undefined }));
vi.mock("./message-budget.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./message-budget.js")>();
  return {
    ...actual,
    fitMessageImages: (capture: Parameters<typeof actual.fitMessageImages>[0]) =>
      actual.fitMessageImages(capture, budget.maxBytes),
    assertMessageBudget: (capture: unknown) => actual.assertMessageBudget(capture, budget.maxBytes),
  };
});

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
  budget.maxBytes = undefined;
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
  capture?: PageCapture | CaptureFailure | null,
  url = "https://example.com/page",
  unreachable = false,
  currentUrl = url,
  navigateAtRead = 1,
) {
  vi.resetModules();
  let runtimeListener: RuntimeListener | undefined;
  const event = () => ({ addListener: vi.fn() });
  const tab = { id: 42, url, title: "Tab title" };
  let reads = 0;
  const requests: SavePageRequest[] = [];
  const sendMessage = vi.fn(async (_tabId: number, message: { action: string }) => {
    if (message.action !== "capture-page") return undefined;
    if (unreachable) throw new Error("The content script could not be reached.");
    return capture;
  });
  vi.stubGlobal("chrome", {
    action: {
      onClicked: event(),
      setIcon: vi.fn(),
      setBadgeText: vi.fn(),
      setBadgeBackgroundColor: vi.fn(),
    },
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
    tabs: {
      onActivated: event(),
      onUpdated: event(),
      get: async () => (reads++ < navigateAtRead ? tab : { ...tab, url: currentUrl }),
      sendMessage,
    },
  });
  await import("./background.js");
  const response = await new Promise((resolve) => {
    runtimeListener!({ action: "toolbar-save", kind: "page", tabId: 42 }, {}, resolve);
  });
  return { requests, sendMessage, response };
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
    expect(result.response).toMatchObject({
      accepted: false,
      error: expect.stringContaining("regular web page"),
    });
  });

  it("does not save a capture returned for a different source URL", async () => {
    const capture = capturedPage();
    capture.metadata.url = "https://example.com/different";
    const result = await harness(capture);
    expect(result.requests).toHaveLength(0);
    expect(result.sendMessage).toHaveBeenCalledWith(
      42,
      expect.objectContaining({ status: "error", detail: expect.stringContaining("page changed") }),
      { frameId: 0 },
    );
  });

  it("does not persist old tab metadata after the tab navigates", async () => {
    const result = await harness(
      undefined,
      "https://example.com/page",
      false,
      "https://example.com/new",
    );
    expect(result.requests).toHaveLength(0);
  });

  it("does not fall back to a bookmark when a capture exceeds the message budget", async () => {
    const result = await harness({
      error: "This page is too large to save.",
      error_code: "capture_too_large",
    });
    expect(result.requests).toHaveLength(0);
    expect(result.response).toMatchObject({
      accepted: false,
      error: "This page is too large to save.",
    });
  });

  it("rechecks the page after asset completion and before native persistence", async () => {
    const result = await harness(
      capturedPage(),
      "https://example.com/page",
      false,
      "https://example.com/new",
      2,
    );
    expect(result.requests).toHaveLength(0);
    expect(result.sendMessage).toHaveBeenCalledWith(
      42,
      expect.objectContaining({ status: "error", detail: expect.stringContaining("page changed") }),
      { frameId: 0 },
    );
  });

  it("rejects oversized text before calling native messaging instead of saving a truncated page", async () => {
    budget.maxBytes = 100;
    const result = await harness(capturedPage());
    expect(result.requests).toHaveLength(0);
    expect(result.response).toMatchObject({
      accepted: false,
      error: "This page is too large to save completely.",
    });
  });

  it("fits optional images in the final native envelope while retaining the complete text", async () => {
    budget.maxBytes = 1_024;
    const capture = capturedPage();
    const url = "https://example.com/preview.png";
    capture.images = [{ url, content_type: "image/png", data_base64: "A".repeat(2_000) }];
    capture.preview_candidates = [url];
    const result = await harness(capture);
    expect(result.requests).toHaveLength(1);
    expect(result.requests[0]).toMatchObject({ markdown: capture.markdown, images: [] });
    expect(result.requests[0]).not.toHaveProperty("preview_url");
    expect(result.response).toEqual({ accepted: true });
  });
});
