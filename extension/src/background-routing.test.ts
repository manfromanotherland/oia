// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";
import { CONTEXT_MENU_ID } from "./context-menus.js";

vi.mock("./log.js", () => ({ log: async () => undefined }));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function harness() {
  vi.resetModules();
  let click: ((info: chrome.contextMenus.OnClickData, tab?: chrome.tabs.Tab) => void) | undefined;
  const event = () => ({ addListener: vi.fn() });
  const sendMessage = vi.fn(async (_tab: number, message: { action: string }) => {
    if (message.action === "capture-media")
      return {
        video_import: true,
        metadata: {
          kind: "video",
          title: "Embedded movie",
          canonical_url: "https://top.example/page",
        },
        response: { ok: true, id: "video", path: "article.md", protocol_version: 4 },
      };
    return undefined;
  });
  const executeScript = vi.fn(async () => []);
  vi.stubGlobal("chrome", {
    action: { setIcon: vi.fn(), setBadgeText: vi.fn(), setBadgeBackgroundColor: vi.fn() },
    commands: { onCommand: event() },
    contextMenus: {
      onClicked: {
        addListener: (listener: typeof click) => {
          click = listener;
        },
      },
      removeAll: vi.fn(),
      create: vi.fn(),
    },
    notifications: { onClicked: event(), create: vi.fn(), clear: vi.fn() },
    runtime: { onInstalled: event(), onStartup: event(), onConnect: event(), onMessage: event() },
    scripting: { executeScript },
    tabs: { sendMessage, onActivated: event(), onUpdated: event() },
  });
  const background = await import("./background.js");
  return { click: click!, sendMessage, executeScript, background };
}

describe("document-targeted capture", () => {
  it("captures an iframe video in its owning frame while retaining the top-level origin", async () => {
    const mock = await harness();
    mock.click(
      {
        menuItemId: CONTEXT_MENU_ID,
        mediaType: "video",
        srcUrl: "blob:https://frame.example/movie",
        frameId: 13,
        pageUrl: "https://top.example/page",
        editable: false,
      },
      { id: 42, url: "https://top.example/page" } as chrome.tabs.Tab,
    );
    await vi.waitFor(() =>
      expect(mock.sendMessage).toHaveBeenCalledWith(
        42,
        {
          action: "capture-media",
          kind: "video",
          mediaUrl: "blob:https://frame.example/movie",
          pageUrl: "https://top.example/page",
        },
        { frameId: 13 },
      ),
    );
    await vi.waitFor(() =>
      expect(mock.sendMessage).toHaveBeenCalledWith(42, expect.objectContaining({ status: "ok" }), {
        frameId: 0,
      }),
    );
  });

  it("coalesces concurrent fallback injections for the same tab and frame", async () => {
    const mock = await harness();
    let injected = false;
    let complete: () => void = () => undefined;
    mock.sendMessage.mockImplementation(async () => {
      if (!injected) throw new Error("No receiver");
      return undefined;
    });
    mock.executeScript.mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = () => {
            injected = true;
            resolve([]);
          };
        }),
    );
    const first = mock.background.requestContentResponse(42, { action: "extract" }, 13);
    const second = mock.background.requestContentResponse(42, { action: "extract" }, 13);
    await vi.waitFor(() => expect(mock.executeScript).toHaveBeenCalledTimes(1));
    complete();
    await Promise.all([first, second]);
    expect(mock.executeScript).toHaveBeenCalledWith({
      target: { tabId: 42, frameIds: [13] },
      files: ["dist/content.js"],
    });
  });
});
