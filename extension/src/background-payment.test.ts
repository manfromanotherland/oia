// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

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

describe("payment-required saves", () => {
  it.each(["article", "link"])("rejects a %s save before capture or persistence", async (kind) => {
    vi.resetModules();
    const runtimeMessageListeners: RuntimeMessageListener[] = [];
    const nativeRequests: object[] = [];
    const tab = { id: 42, url: "https://example.com/article" };
    const sendMessage = vi.fn(async () => undefined);
    const visibleCaptureTimes: number[] = [];
    const chromeMock = {
      action: {
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
            callback({ protocol_version: 4, ok: true, id: "reading-1", path: "article.md" });
          },
        ),
        lastError: undefined,
      },
      scripting: { executeScript: vi.fn(async () => [{ result: 402 }]) },
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

    await import("./background.js");
    await new Promise((resolve) => {
      runtimeMessageListeners[0]({ action: "toolbar-save", kind, tabId: 42 }, {}, resolve);
    });
    expect(nativeRequests).toHaveLength(0);
    expect(sendMessage).toHaveBeenCalledWith(
      42,
      expect.objectContaining({
        action: "toast",
        status: "error",
        title: "Payment required",
      }),
    );
    expect(sendMessage.mock.calls).toHaveLength(1);
  });
});
