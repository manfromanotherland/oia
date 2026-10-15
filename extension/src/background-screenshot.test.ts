// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  SCREENSHOT_PAGE_BEGIN_ACTION,
  SCREENSHOT_PAGE_FINISH_ACTION,
  SCREENSHOT_PAGE_MEASURE_ACTION,
  SCREENSHOT_PAGE_SCROLL_ACTION,
} from "./screenshot-page.js";

type RuntimeMessageListener = (
  message: unknown,
  sender: chrome.runtime.MessageSender,
  sendResponse: (response: unknown) => void,
) => boolean | void;

function event<T extends (...args: never[]) => unknown>(listeners?: T[]): object {
  return { addListener: vi.fn((listener: T) => listeners?.push(listener)) };
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("background screenshot save", () => {
  it("returns capture failures to the popup when the restricted page cannot render a toast", async () => {
    vi.resetModules();
    const listeners: RuntimeMessageListener[] = [];
    const tab = { id: 42, url: "https://chromewebstore.google.com/detail/extension", windowId: 7 };
    const sendNativeMessage = vi.fn();
    vi.stubGlobal("chrome", {
      action: {
        onClicked: event(),
        setIcon: vi.fn(),
        setBadgeText: vi.fn(),
        setBadgeBackgroundColor: vi.fn(),
      },
      commands: { onCommand: event() },
      contextMenus: { onClicked: event(), removeAll: vi.fn(), create: vi.fn() },
      notifications: { onClicked: event() },
      storage: { local: { get: vi.fn(async () => ({})), set: vi.fn(async () => undefined) } },
      runtime: {
        onInstalled: event(),
        onStartup: event(),
        onConnect: event(),
        onMessage: event(listeners),
        sendNativeMessage,
      },
      tabs: {
        onActivated: event(),
        onUpdated: event(),
        get: vi.fn(async () => tab),
        sendMessage: vi.fn(async () => {
          throw new Error("Cannot access this page.");
        }),
      },
      scripting: {
        executeScript: vi.fn(async () => {
          throw new Error("Cannot access this page.");
        }),
      },
    });
    await import("./background.js");
    const response = await new Promise((resolve) => {
      listeners[0]({ action: "toolbar-save", kind: "screenshot", tabId: 42 }, {}, resolve);
    });
    expect(response).toEqual({ accepted: false, error: "Cannot access this page." });
    expect(sendNativeMessage).not.toHaveBeenCalled();
  });

  it("shows saving feedback after restoring the page, through PNG assembly and native persistence", async () => {
    vi.resetModules();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-10T12:00:00.000Z"));

    const runtimeMessageListeners: RuntimeMessageListener[] = [];
    const pageActions: string[] = [];
    const scrollOffsets: number[] = [];
    const visibleCaptureTimes: number[] = [];
    const nativeRequests: object[] = [];
    const toasts: { status?: string; title?: string }[] = [];
    const milestones: string[] = [];
    let finishEncoding: (blob: Blob) => void = () => undefined;
    const encodedPng = new Promise<Blob>((resolve) => {
      finishEncoding = resolve;
    });
    let finishPersistence: () => void = () => undefined;
    const tab: chrome.tabs.Tab = {
      id: 42,
      index: 0,
      pinned: false,
      highlighted: true,
      active: true,
      incognito: false,
      selected: true,
      discarded: false,
      autoDiscardable: true,
      groupId: -1,
      windowId: 7,
      url: "https://example.com/long-page",
      title: "Long page",
    };
    let scrollY = 250;

    const sendMessage = vi.fn(
      async (_tabId: number, message: { action?: string; status?: string; title?: string }) => {
        if (message.action === "toast") {
          toasts.push({ status: message.status, title: message.title });
          milestones.push(`toast:${message.status}`);
          return undefined;
        }
        if (!message.action) return undefined;
        pageActions.push(message.action);
        if (message.action === SCREENSHOT_PAGE_BEGIN_ACTION) {
          return pageState(scrollY);
        }
        if (message.action === SCREENSHOT_PAGE_SCROLL_ACTION) {
          const requested = (message as { scrollY: number }).scrollY;
          scrollY = Math.min(requested, 900);
          scrollOffsets.push(scrollY);
          return pageState(scrollY);
        }
        if (message.action === SCREENSHOT_PAGE_MEASURE_ACTION) return pageState(scrollY);
        if (message.action === SCREENSHOT_PAGE_FINISH_ACTION) {
          scrollY = 250;
          milestones.push("restored");
          return { ok: true };
        }
        throw new Error(`Unexpected page action: ${message.action}`);
      },
    );

    const chromeMock = {
      action: {
        onClicked: event(),
        setIcon: vi.fn(async () => undefined),
        setBadgeText: vi.fn(async () => undefined),
        setBadgeBackgroundColor: vi.fn(async () => undefined),
      },
      commands: { onCommand: event() },
      contextMenus: {
        onClicked: event(),
        removeAll: vi.fn(),
        create: vi.fn(),
      },
      notifications: {
        onClicked: event(),
        create: vi.fn(async () => undefined),
        clear: vi.fn(async () => true),
      },
      runtime: {
        onInstalled: event(),
        onStartup: event(),
        onConnect: event(),
        onMessage: event(runtimeMessageListeners),
        getURL: (path: string) => `chrome-extension://oia/${path}`,
        sendNativeMessage: vi.fn(
          (_host: string, request: object, callback: (response: object) => void) => {
            nativeRequests.push(request);
            milestones.push("persisting");
            finishPersistence = () =>
              callback({ protocol_version: 4, ok: true, id: "reading-1", path: "article.md" });
          },
        ),
        lastError: undefined,
      },
      scripting: { executeScript: vi.fn(async () => undefined) },
      storage: {
        local: {
          get: vi.fn(async () => ({})),
          set: vi.fn(async () => undefined),
          remove: vi.fn(async () => undefined),
        },
      },
      tabs: {
        onActivated: event(),
        onUpdated: event(),
        get: vi.fn(async () => tab),
        query: vi.fn(async () => [tab]),
        sendMessage,
        captureVisibleTab: vi.fn(async () => {
          visibleCaptureTimes.push(Date.now());
          return "data:image/png;base64,AQID";
        }),
        create: vi.fn(async () => tab),
      },
    };
    vi.stubGlobal("chrome", chromeMock);
    vi.stubGlobal(
      "createImageBitmap",
      vi.fn(async () => ({ width: 1_600, height: 1_200, close: vi.fn() })),
    );
    vi.stubGlobal(
      "OffscreenCanvas",
      class {
        getContext(): object {
          return { drawImage: vi.fn(), imageSmoothingEnabled: false, imageSmoothingQuality: "low" };
        }

        async convertToBlob(): Promise<Blob> {
          milestones.push("encoding");
          return encodedPng;
        }
      },
    );

    await import("./background.js");
    expect(runtimeMessageListeners).toHaveLength(1);

    const response = new Promise<unknown>((resolve) => {
      const keepOpen = runtimeMessageListeners[0](
        { action: "toolbar-save", kind: "screenshot", tabId: 42 },
        {},
        resolve,
      );
      expect(keepOpen).toBe(true);
    });
    await vi.runAllTimersAsync();

    expect(scrollY).toBe(250);
    expect(nativeRequests).toHaveLength(0);
    expect(milestones).toEqual(["restored", "toast:loading", "encoding"]);
    expect(toasts).toEqual([{ status: "loading", title: "Saving screenshot…" }]);

    finishEncoding(new Blob([Uint8Array.of(1, 2, 3).buffer], { type: "image/png" }));
    await vi.waitFor(() => expect(nativeRequests).toHaveLength(1));
    expect(toasts).toEqual([{ status: "loading", title: "Saving screenshot…" }]);
    finishPersistence();
    await expect(response).resolves.toEqual({ accepted: true });
    expect(toasts).toEqual([
      { status: "loading", title: "Saving screenshot…" },
      { status: "ok", title: "Screenshot saved" },
    ]);
    expect(scrollOffsets).toEqual([0, 600, 900]);
    expect(chromeMock.tabs.captureVisibleTab).toHaveBeenCalledTimes(3);
    expect(visibleCaptureTimes.map((time) => time - visibleCaptureTimes[0])).toEqual([
      0, 550, 1_100,
    ]);
    expect(pageActions).toEqual([
      SCREENSHOT_PAGE_BEGIN_ACTION,
      SCREENSHOT_PAGE_SCROLL_ACTION,
      SCREENSHOT_PAGE_MEASURE_ACTION,
      SCREENSHOT_PAGE_SCROLL_ACTION,
      SCREENSHOT_PAGE_MEASURE_ACTION,
      SCREENSHOT_PAGE_SCROLL_ACTION,
      SCREENSHOT_PAGE_MEASURE_ACTION,
      SCREENSHOT_PAGE_FINISH_ACTION,
    ]);
    expect(scrollY).toBe(250);
    expect(nativeRequests).toHaveLength(1);
    expect(nativeRequests[0]).toMatchObject({
      protocol_version: 4,
      action: "save",
      metadata: {
        kind: "image",
        url: "https://example.com/long-page",
      },
      images: [{ content_type: "image/png", data_base64: "AQID" }],
    });

    function pageState(currentScrollY: number): object {
      return {
        sessionId: "session-1",
        url: tab.url,
        viewportWidth: 800,
        viewportHeight: 600,
        documentHeight: 1_500,
        scrollY: currentScrollY,
      };
    }
  });
});
