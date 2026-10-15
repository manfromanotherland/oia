// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import type { PageCapture } from "./content.js";

vi.mock("./log.js", () => ({ log: async () => undefined }));
vi.mock("./full-page-screenshot.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./full-page-screenshot.js")>()),
  captureFullPageScreenshot: vi.fn(
    async (dependencies: import("./full-page-screenshot.js").FullPageScreenshotDependencies) =>
      dependencies.compose({
        documentHeight: 600,
        viewportWidth: 800,
        viewportHeight: 600,
        tiles: [],
      }),
  ),
  composeFullPageScreenshot: vi.fn(async () => "data:image/png;base64,AQID"),
}));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function harness(deferNativeSave = false) {
  vi.resetModules();
  let click: (tab: chrome.tabs.Tab) => void = () => undefined;
  let choose: (choice: { kind: "page" | "screenshot" | "cancel" }) => void = () => undefined;
  let rejectChoice: (error: Error) => void = () => undefined;
  let finishSave: (response?: object) => void = () => undefined;
  const event = () => ({ addListener: vi.fn() });
  const tab = { id: 42, url: "https://example.com/page", title: "Page title", windowId: 7 };
  let currentUrl = tab.url;
  const capture: PageCapture = {
    metadata: {
      kind: "article",
      url: tab.url,
      canonical_url: tab.url,
      title: tab.title,
      saved_at: "2026-10-15T12:00:00Z",
    },
    markdown: "Page content.",
    images: [],
    unresolved: [],
  };
  const nativeRequests: { action: string; metadata?: { kind: string } }[] = [];
  const sendMessage = vi.fn(async (_tabId: number, message: { action: string }) => {
    if (message.action === "save-choice") {
      return new Promise<{ kind: "page" | "screenshot" | "cancel" }>((resolve, reject) => {
        choose = resolve;
        rejectChoice = reject;
      });
    }
    if (message.action === "capture-page") return capture;
    return undefined;
  });
  vi.stubGlobal("chrome", {
    action: {
      onClicked: { addListener: (listener: typeof click) => (click = listener) },
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
      onMessage: event(),
      sendNativeMessage: (
        _host: string,
        request: (typeof nativeRequests)[number],
        callback: (response: object) => void,
      ) => {
        if (request.action === "inspect_page_metadata") {
          callback({ protocol_version: 4, ok: true, preview_candidates: [] });
          return;
        }
        nativeRequests.push(request);
        const response = { protocol_version: 4, ok: true, id: "saved", path: "article.md" };
        if (deferNativeSave) finishSave = (result = response) => callback(result);
        else callback(response);
      },
    },
    scripting: { executeScript: vi.fn(async () => [{ result: false }]) },
    tabs: {
      onActivated: event(),
      onUpdated: event(),
      get: vi.fn(async () => ({ ...tab, url: currentUrl })),
      query: vi.fn(async () => [{ ...tab, url: currentUrl }]),
      sendMessage,
    },
  });
  await import("./background.js");
  const startedAt = Date.now();
  click(tab as chrome.tabs.Tab);
  await vi.waitFor(() =>
    expect(sendMessage).toHaveBeenCalledWith(
      42,
      expect.objectContaining({ action: "save-choice" }),
      { frameId: 0 },
    ),
  );
  return {
    nativeRequests,
    sendMessage,
    startedAt,
    finishSave: (response?: object) => finishSave(response),
    choose: (kind: "page" | "screenshot" | "cancel") => choose({ kind }),
    disconnect: () => {
      sendMessage.mockRejectedValue(new Error("The source document closed."));
      rejectChoice(new Error("The source document closed."));
    },
    navigate: () => (currentUrl = "https://example.com/other"),
  };
}

describe("toolbar save countdown", () => {
  it("opens the in-page choice immediately and writes nothing before the three-second decision", async () => {
    const mock = await harness();
    const message = mock.sendMessage.mock.calls[0][1];
    expect(message).toMatchObject({
      action: "save-choice",
      title: "Saving…",
      cta: { label: "Screenshot instead?", command: "save-screenshot" },
    });
    const expiresAt = (message as unknown as { cta: { expiresAt: number } }).cta.expiresAt;
    expect(expiresAt - mock.startedAt).toBeGreaterThanOrEqual(3000);
    expect(expiresAt - mock.startedAt).toBeLessThan(3100);
    expect(mock.nativeRequests).toHaveLength(0);
    expect(mock.sendMessage.mock.calls.some(([, sent]) => sent.action === "capture-page")).toBe(
      false,
    );
    mock.choose("page");
    await vi.waitFor(() => expect(mock.nativeRequests).toHaveLength(1));
    expect(mock.nativeRequests[0].action).toBe("save_page");
  });

  it("saves only a screenshot and keeps saving feedback until persistence completes", async () => {
    const mock = await harness(true);
    mock.choose("screenshot");
    await vi.waitFor(() => expect(mock.nativeRequests).toHaveLength(1));
    expect(mock.nativeRequests[0]).toMatchObject({ action: "save", metadata: { kind: "image" } });
    expect(mock.sendMessage.mock.calls.some(([, sent]) => sent.action === "capture-page")).toBe(
      false,
    );
    expect(mock.sendMessage.mock.calls.filter(([, sent]) => sent.action === "toast")).toEqual([
      [
        42,
        expect.objectContaining({ status: "loading", title: "Saving screenshot…" }),
        { frameId: 0 },
      ],
    ]);
    mock.finishSave();
    await vi.waitFor(() =>
      expect(mock.sendMessage).toHaveBeenCalledWith(
        42,
        expect.objectContaining({ action: "toast", status: "ok", title: "Screenshot saved" }),
        { frameId: 0 },
      ),
    );
    expect(mock.sendMessage.mock.calls.filter(([, sent]) => sent.action === "toast")).toHaveLength(
      2,
    );
  });

  it("replaces screenshot saving feedback with a persistence error", async () => {
    const mock = await harness(true);
    mock.choose("screenshot");
    await vi.waitFor(() => expect(mock.nativeRequests).toHaveLength(1));
    mock.finishSave({ protocol_version: 4, ok: false, error: "io_error", message: "Disk full." });
    await vi.waitFor(() =>
      expect(mock.sendMessage).toHaveBeenCalledWith(
        42,
        expect.objectContaining({ status: "error", title: "Couldn't save screenshot" }),
        { frameId: 0 },
      ),
    );
    expect(mock.sendMessage.mock.calls.filter(([, sent]) => sent.action === "toast")).toHaveLength(
      2,
    );
  });

  it("does not save a stale choice after another toast replaces it", async () => {
    const mock = await harness();
    mock.choose("cancel");
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(mock.nativeRequests).toHaveLength(0);
  });

  it("does not save a different page if the source navigates during the countdown", async () => {
    const mock = await harness();
    mock.navigate();
    mock.choose("page");
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(mock.nativeRequests).toHaveLength(0);
  });

  it("does not turn a disconnected countdown into an immediate save", async () => {
    const mock = await harness();
    mock.disconnect();
    await vi.waitFor(() =>
      expect(mock.sendMessage).toHaveBeenCalledWith(
        42,
        expect.objectContaining({ status: "error" }),
        { frameId: 0 },
      ),
    );
    expect(mock.nativeRequests).toHaveLength(0);
  });
});
