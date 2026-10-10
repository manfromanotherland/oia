// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import type { SaveSelectionRequest } from "./protocol.js";
import type { SelectionCapture } from "./selection-capture.js";
import { CONTEXT_MENU_ID } from "./context-menus.js";

vi.mock("./log.js", () => ({ log: async () => undefined }));

type ContextListener = (info: chrome.contextMenus.OnClickData, tab?: chrome.tabs.Tab) => void;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function harness(selectionKind: "highlight" | "quote" = "highlight", retryFrame = false) {
  vi.resetModules();
  let contextListener: ContextListener | undefined;
  const event = () => ({ addListener: vi.fn() });
  const capture: SelectionCapture = {
    metadata: {
      kind: "article",
      url: "https://frame.example/article",
      canonical_url: "https://frame.example/article",
      title: "Source article",
      saved_at: "2026-10-10T12:00:00Z",
    },
    markdown: "Full saved article text.",
    selected_text: "Selected passage",
    selection_context: {
      version: 1,
      selected_text: "Selected passage",
      enclosing_element: "blockquote",
    },
    images: [],
    unresolved: [],
  };
  let requestedFrame = false;
  const sendMessage = vi.fn(
    async (_tabId: number, message: { action: string }, _options: { frameId: number }) => {
      if (message.action === "capture-selection") {
        if (retryFrame && !requestedFrame) {
          requestedFrame = true;
          throw new Error("No content script");
        }
        return capture;
      }
      return undefined;
    },
  );
  const sendNativeMessage = vi.fn(
    (_host: string, _request: SaveSelectionRequest, callback: (response: object) => void) => {
      callback({
        protocol_version: 4,
        ok: true,
        id: "selected-1",
        path: "article.md",
        selection_kind: selectionKind,
      });
    },
  );
  const executeScript = vi.fn(async () => undefined);
  const chromeMock = {
    action: {
      setIcon: vi.fn(async () => undefined),
      setBadgeText: vi.fn(async () => undefined),
      setBadgeBackgroundColor: vi.fn(async () => undefined),
    },
    commands: { onCommand: event() },
    contextMenus: {
      onClicked: {
        addListener: (listener: ContextListener) => {
          contextListener = listener;
        },
      },
      removeAll: vi.fn(),
      create: vi.fn(),
    },
    notifications: { onClicked: event(), create: vi.fn(), clear: vi.fn() },
    runtime: {
      onInstalled: event(),
      onStartup: event(),
      onConnect: event(),
      onMessage: event(),
      sendNativeMessage,
      lastError: undefined,
    },
    scripting: { executeScript },
    tabs: { sendMessage, onActivated: event(), onUpdated: event() },
  };
  vi.stubGlobal("chrome", chromeMock);
  const background = await import("./background.js");
  const tab = { id: 42, url: "https://top.example/page", title: "Top page" } as chrome.tabs.Tab;
  return {
    background,
    tab,
    contextListener: contextListener!,
    sendMessage,
    sendNativeMessage,
    executeScript,
    chromeMock,
  };
}

describe("background selection save", () => {
  it.each(["highlight", "quote"] as const)(
    "routes the selected frame to save_selection and displays the core's %s result",
    async (selectionKind) => {
      const mock = await harness(selectionKind);
      mock.contextListener(
        {
          menuItemId: CONTEXT_MENU_ID,
          selectionText: "Selected passage",
          frameId: 13,
          frameUrl: "https://frame.example/article",
          pageUrl: "https://top.example/page",
          editable: false,
        },
        mock.tab,
      );
      await vi.waitFor(() => expect(mock.sendNativeMessage).toHaveBeenCalledTimes(1));
      await vi.waitFor(() =>
        expect(mock.sendMessage).toHaveBeenCalledWith(
          42,
          expect.objectContaining({ status: "ok", title: `Saved ${selectionKind} to Óia` }),
          { frameId: 0 },
        ),
      );
      expect(mock.sendMessage).toHaveBeenCalledWith(
        42,
        {
          action: "capture-selection",
          pageUrl: "https://frame.example/article",
          text: "Selected passage",
        },
        { frameId: 13 },
      );
      expect(mock.sendNativeMessage.mock.calls[0][1]).toMatchObject({
        protocol_version: 4,
        action: "save_selection",
        selected_text: "Selected passage",
        selection_context: { version: 1, enclosing_element: "blockquote" },
        markdown: "Full saved article text.",
        metadata: { kind: "article", url: "https://frame.example/article" },
      });
      expect(mock.chromeMock.action.setIcon).not.toHaveBeenCalled();
      expect(mock.sendMessage).toHaveBeenCalledWith(
        42,
        expect.objectContaining({ title: "Saving selection…" }),
        { frameId: 0 },
      );
    },
  );

  it("injects a missing content script into the clicked frame before retrying", async () => {
    const mock = await harness("highlight", true);
    mock.contextListener(
      {
        menuItemId: CONTEXT_MENU_ID,
        selectionText: "Selected passage",
        frameId: 13,
        frameUrl: "https://frame.example/article",
        pageUrl: "https://top.example/page",
        editable: false,
      },
      mock.tab,
    );
    await vi.waitFor(() => expect(mock.sendNativeMessage).toHaveBeenCalledTimes(1));
    expect(mock.executeScript).toHaveBeenCalledWith({
      target: { tabId: 42, frameIds: [13] },
      files: ["dist/content.js"],
    });
    const captures = mock.sendMessage.mock.calls.filter(
      (call) => call[1].action === "capture-selection",
    );
    expect(captures).toHaveLength(2);
    expect(captures.every((call) => call[2].frameId === 13)).toBe(true);
  });

  it("keeps ordinary page capture explicitly in the top frame", async () => {
    const mock = await harness();
    await mock.background.requestContentResponse(42, { action: "extract" });
    expect(mock.sendMessage).toHaveBeenCalledWith(42, { action: "extract" }, { frameId: 0 });
  });
});
