// SPDX-License-Identifier: MIT

import type {
  ImportedVideoCapture,
  PageCapture,
  StandaloneMediaCaptureResponse,
  ToastMessage,
} from "./content.js";
import { CONTEXT_MENU, CONTEXT_MENU_ID, contextCaptureTarget } from "./context-menus.js";
import type {
  CheckRequest,
  CheckResponse,
  ImageData,
  SaveKind,
  SaveLinkRequest,
  SaveRequest,
  SaveResponse,
} from "./protocol.js";
import { PROTOCOL_VERSION } from "./protocol.js";
import { capTotalBytes, fetchImages } from "./images.js";
import { HOST_ID, isHostMissing } from "./host.js";
import { log } from "./log.js";
import { isSessionLocalVideoUrl } from "./media.js";
import { isPreparePageVideoBridgeMessage, PREPARE_PAGE_VIDEO_BRIDGE } from "./page-video-source.js";
import {
  captureFullPageScreenshot,
  composeFullPageScreenshot,
  MINIMUM_SCREENSHOT_CAPTURE_INTERVAL_MS,
  type ScreenshotPageState,
} from "./full-page-screenshot.js";
import {
  SCREENSHOT_PAGE_BEGIN_ACTION,
  SCREENSHOT_PAGE_FINISH_ACTION,
  SCREENSHOT_PAGE_MEASURE_ACTION,
  SCREENSHOT_PAGE_SCROLL_ACTION,
  type ScreenshotPageBeginMessage,
  type ScreenshotPageFinishMessage,
  type ScreenshotPageMeasureMessage,
  type ScreenshotPageResponse,
  type ScreenshotPageScrollMessage,
} from "./screenshot-page.js";
import { relayVideoImportPort, type RelayPort } from "./video-import-relay.js";
import { VIDEO_IMPORT_PORT_NAME } from "./video-import.js";
import {
  buildFallbackLinkCapture,
  buildSaveLinkRequest,
  buildScreenshotCapture,
  isStableScreenshotDocument,
  isToolbarSaveMessage,
  type ToolbarSaveKind,
} from "./toolbar.js";

const NOTIF_HOST_MISSING = "host-missing";

/** Cap on the total decoded image bytes inlined into one save message. Images
 *  beyond this stay as remote-URL placeholders so a save can't buffer unbounded. */
const MAX_TOTAL_IMAGE_BYTES = 40 * 1024 * 1024;
let activeScreenshotTabId: number | undefined;
let lastVisibleTabCaptureStartedAt: number | undefined;

// ── Icon ──────────────────────────────────────────────────────────────────────

type IconState = "default" | "saved";

/** The saved state is the same artwork carrying a green check badge. */
function iconPaths(state: IconState): Record<number, string> {
  const stem = state === "saved" ? "/icons/icon-saved" : "/icons/icon";
  return { 16: `${stem}-16.png`, 32: `${stem}-32.png`, 48: `${stem}-48.png` };
}

async function setIcon(state: IconState = "default", tabId?: number): Promise<void> {
  const path = iconPaths(state);
  if (tabId !== undefined) {
    await chrome.action.setIcon({ path, tabId });
  } else {
    await chrome.action.setIcon({ path });
  }
}

// ── Saved-URL cache ───────────────────────────────────────────────────────────

/** In-memory cache: normalized URL → is it saved? Populated by check and save. */
const savedCache = new Map<string, boolean>();

function normalizeUrl(raw: string): string {
  return raw.split("#")[0];
}

// ── Tab icon check ────────────────────────────────────────────────────────────

async function checkAndSetIcon(tabId: number, rawUrl: string): Promise<void> {
  if (!rawUrl || rawUrl.startsWith("chrome://") || rawUrl.startsWith("about:")) return;

  const url = normalizeUrl(rawUrl);

  if (savedCache.has(url)) {
    await setIcon(savedCache.get(url)! ? "saved" : "default", tabId);
    return;
  }

  try {
    const response = await sendNativeMessage<CheckRequest, CheckResponse>({
      protocol_version: PROTOCOL_VERSION,
      action: "check",
      url,
    });
    const isSaved = response.saved ?? false;
    savedCache.set(url, isSaved);
    await setIcon(isSaved ? "saved" : "default", tabId);
  } catch {
    // Native host not installed or unreachable — silently skip.
  }
}

// ── Startup ───────────────────────────────────────────────────────────────────

chrome.runtime.onInstalled.addListener(() => {
  void setIcon();
  // An update may leave the upstream menu registered. Rebuild our menu set so
  // the user sees exactly one generic action for every supported context.
  chrome.contextMenus.removeAll(() => chrome.contextMenus.create(CONTEXT_MENU));
});

chrome.runtime.onStartup.addListener(() => {
  void setIcon();
});

// ── Tab navigation listeners ──────────────────────────────────────────────────

chrome.tabs.onActivated.addListener(({ tabId }) => {
  void chrome.tabs.get(tabId, (tab) => {
    if (tab.url) void checkAndSetIcon(tabId, tab.url);
  });
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (changeInfo.status === "complete" && tab.url) {
    void checkAndSetIcon(tabId, tab.url);
  }
});

// ── Save pipeline ─────────────────────────────────────────────────────────────

/**
 * Ask the content script to extract the page. The content script is normally
 * injected by the manifest, but only into tabs that navigate *after* the
 * extension loads — a tab that was already open when the extension was
 * installed or reloaded has no content script and won't answer. When the first
 * message fails, inject the script programmatically (allowed by the `<all_urls>`
 * host permission) and retry once.
 */
async function requestContentResponse<T>(tabId: number, message: object): Promise<T> {
  try {
    return (await chrome.tabs.sendMessage(tabId, message)) as T;
  } catch {
    await chrome.scripting.executeScript({ target: { tabId }, files: ["dist/content.js"] });
    return (await chrome.tabs.sendMessage(tabId, message)) as T;
  }
}

function requestExtraction(tabId: number): Promise<PageCapture | { error: string }> {
  return requestContentResponse(tabId, { action: "extract" });
}

function requestLinkCapture(
  tabId: number,
  pageUrl: string,
): Promise<PageCapture | { error: string }> {
  return requestContentResponse(tabId, { action: "capture-link", pageUrl });
}

function requestMediaCapture(
  tabId: number,
  pageUrl: string,
  kind: "image" | "video",
  mediaUrl: string,
): Promise<StandaloneMediaCaptureResponse> {
  return requestContentResponse(tabId, {
    action: "capture-media",
    kind,
    mediaUrl,
    pageUrl,
  });
}

function requestQuoteCapture(
  tabId: number,
  pageUrl: string,
  text: string,
): Promise<PageCapture | { error: string }> {
  return requestContentResponse(tabId, {
    action: "capture-quote",
    pageUrl,
    text,
  });
}

async function requestScreenshotPageState(
  tabId: number,
  message: ScreenshotPageBeginMessage | ScreenshotPageScrollMessage | ScreenshotPageMeasureMessage,
): Promise<ScreenshotPageState> {
  const response = await requestContentResponse<ScreenshotPageResponse>(tabId, message);
  if (!response || typeof response !== "object") {
    throw new Error("The page returned an invalid screenshot response.");
  }
  if ("error" in response) throw new Error(response.error);
  if (!("sessionId" in response)) {
    throw new Error("The page returned an invalid screenshot response.");
  }
  return response;
}

async function finishScreenshotPage(tabId: number, sessionId: string): Promise<void> {
  const message: ScreenshotPageFinishMessage = {
    action: SCREENSHOT_PAGE_FINISH_ACTION,
    sessionId,
  };
  const response = await requestContentResponse<ScreenshotPageResponse>(tabId, message);
  if (!response || typeof response !== "object") {
    throw new Error("The page returned an invalid screenshot response.");
  }
  if ("error" in response) throw new Error(response.error);
  if (!("ok" in response) || response.ok !== true) {
    throw new Error("The page did not finish the screenshot session.");
  }
}

/** Read the loaded document's response, without re-fetching subscriber content. */
async function rejectPaymentRequired(tabId: number): Promise<boolean> {
  let blocked = false;
  try {
    const results = await chrome.scripting.executeScript({
      target: { tabId },
      args: [(await chrome.tabs.get(tabId)).url ?? ""],
      func: (pageUrl: string) => {
        const navigation = performance.getEntriesByType("navigation")[0] as
          | (PerformanceEntry & { responseStatus?: number })
          | undefined;
        if (navigation?.responseStatus === 402) return true;
        const host = new URL(pageUrl).hostname;
        if (host !== "economist.com" && host !== "www.economist.com") return false;
        // The Economist also serves a subscription gate in successful responses.
        // Require both gate-specific phrases, rather than a generic Subscribe button.
        const text = (document.body?.innerText ?? document.body?.textContent ?? "").replace(
          /\s+/g,
          " ",
        );
        return (
          /Continue with a free trial/i.test(text) &&
          /(?:unlock just this article|Get full access to our independent journalism for free)/i.test(
            text,
          )
        );
      },
    });
    blocked = results[0]?.result === true;
  } catch {
    // Restricted pages and older browsers may not expose navigation status.
    return false;
  }
  if (!blocked) return false;
  await showBadge(tabId, "error");
  await showToast(tabId, "error", "Payment required", "This page wasn't saved.");
  return true;
}

async function savePage(tab: chrome.tabs.Tab): Promise<void> {
  const tabId = tab.id;
  if (!tabId || !tab.url) {
    await log("warn", "Save ignored: no active tab id or URL", { tabId, url: tab.url });
    return;
  }

  if (await rejectPaymentRequired(tabId)) return;

  await log("info", "Save triggered", { kind: "article", url: tab.url });
  await showToast(tabId, "loading", "Saving…");

  let capture: PageCapture;
  try {
    const result = await requestExtraction(tabId);
    if (result && "error" in result && result.error) {
      await showBadge(tabId, "error");
      await showToast(tabId, "error", "Couldn't save page", result.error);
      await log("error", "Extraction failed", { url: tab.url, error: result.error });
      return;
    }
    capture = result as PageCapture;
    addTabFaviconCandidate(capture, tab.favIconUrl);
  } catch (err) {
    await showBadge(tabId, "error");
    await showToast(
      tabId,
      "error",
      "Couldn't save page",
      "This page can't be read (it may be a browser or store page).",
    );
    await log("error", "Could not reach content script", { url: tab.url, error: err });
    return;
  }

  await saveCapture(tab, capture);
}

async function saveLink(tab: chrome.tabs.Tab): Promise<void> {
  const tabId = tab.id;
  if (!tabId || !tab.url) {
    await log("warn", "Save link ignored: no active tab id or URL", { tabId, url: tab.url });
    return;
  }

  if (await rejectPaymentRequired(tabId)) return;

  await log("info", "Save triggered", { kind: "link", url: tab.url });
  await showToast(tabId, "loading", "Saving link…");

  let capture: PageCapture;
  try {
    const result = await requestLinkCapture(tabId, tab.url);
    if (result && "error" in result && result.error) {
      throw new Error(result.error);
    }
    capture = result as PageCapture;
    addTabFaviconCandidate(capture, tab.favIconUrl);
  } catch (err) {
    await log("warn", "Link preview metadata unavailable; using tab metadata", {
      url: tab.url,
      error: err,
    });
    try {
      capture = buildFallbackLinkCapture(tab);
      addTabFaviconCandidate(capture, tab.favIconUrl);
    } catch (fallbackError) {
      await showBadge(tabId, "error");
      await showToast(
        tabId,
        "error",
        "Couldn't save link",
        fallbackError instanceof Error ? fallbackError.message : "The page URL is not supported.",
      );
      await log("error", "Link fallback request was invalid", {
        url: tab.url,
        error: fallbackError,
      });
      return;
    }
  }

  capture.images = await completeCaptureImages(capture);

  let request: SaveLinkRequest;
  try {
    request = buildSaveLinkRequest(capture);
  } catch (err) {
    await showBadge(tabId, "error");
    await showToast(
      tabId,
      "error",
      "Couldn't save link",
      err instanceof Error ? err.message : "The page URL is not supported.",
    );
    await log("error", "Link request was invalid", { url: tab.url, error: err });
    return;
  }

  await persistSave(tab, request, feedbackFor(capture, "link"));
}

async function saveScreenshot(tab: chrome.tabs.Tab): Promise<void> {
  const tabId = tab.id;
  if (!tabId || !tab.url) {
    await log("warn", "Save screenshot ignored: no active tab id or URL", {
      tabId,
      url: tab.url,
    });
    return;
  }

  if (activeScreenshotTabId !== undefined) {
    await showToast(
      tabId,
      "error",
      "Couldn't save screenshot",
      "A full-page screenshot is already in progress.",
    );
    return;
  }
  activeScreenshotTabId = tabId;

  let capture: PageCapture;
  try {
    await waitForVisibleTabCaptureWindow();
    const dataUrl = await captureFullPageScreenshot({
      begin: () =>
        requestScreenshotPageState(tabId, {
          action: SCREENSHOT_PAGE_BEGIN_ACTION,
        }),
      scrollTo: (sessionId, scrollY, hideFixed) =>
        requestScreenshotPageState(tabId, {
          action: SCREENSHOT_PAGE_SCROLL_ACTION,
          sessionId,
          scrollY,
          hideFixed,
        }),
      captureViewport: () => captureStableViewport(tab),
      measure: (sessionId) =>
        requestScreenshotPageState(tabId, {
          action: SCREENSHOT_PAGE_MEASURE_ACTION,
          sessionId,
        }),
      compose: composeFullPageScreenshot,
      finish: (sessionId) => finishScreenshotPage(tabId, sessionId),
    });
    const [afterCapture] = await chrome.tabs.query({ active: true, windowId: tab.windowId });
    if (!afterCapture || !isStableScreenshotDocument(tab, afterCapture)) {
      throw new Error("The active page changed while the screenshot was being captured.");
    }
    capture = await buildScreenshotCapture(tab, dataUrl);
  } catch (err) {
    await showBadge(tabId, "error");
    await showToast(
      tabId,
      "error",
      "Couldn't save screenshot",
      err instanceof Error ? err.message : "The full page couldn't be captured.",
    );
    await log("error", "Screenshot capture failed", { url: tab.url, error: err });
    return;
  } finally {
    if (activeScreenshotTabId === tabId) activeScreenshotTabId = undefined;
  }

  await log("info", "Save triggered", { kind: "screenshot", url: tab.url });
  await showToast(tabId, "loading", "Saving screenshot…");
  await saveCapture(tab, capture, "screenshot");
}

async function captureStableViewport(tab: chrome.tabs.Tab): Promise<string> {
  await waitForVisibleTabCaptureWindow();
  const [beforeCapture] = await chrome.tabs.query({ active: true, windowId: tab.windowId });
  if (!beforeCapture || !isStableScreenshotDocument(tab, beforeCapture)) {
    throw new Error("The active tab changed before the screenshot could be captured.");
  }

  lastVisibleTabCaptureStartedAt = Date.now();
  const dataUrl = await chrome.tabs.captureVisibleTab(tab.windowId, { format: "png" });
  const [afterCapture] = await chrome.tabs.query({ active: true, windowId: tab.windowId });
  if (
    !afterCapture ||
    !isStableScreenshotDocument(beforeCapture, afterCapture) ||
    !isStableScreenshotDocument(tab, afterCapture)
  ) {
    throw new Error("The active page changed while the screenshot was being captured.");
  }
  return dataUrl;
}

async function waitForVisibleTabCaptureWindow(): Promise<void> {
  if (lastVisibleTabCaptureStartedAt === undefined) return;
  const remaining =
    MINIMUM_SCREENSHOT_CAPTURE_INTERVAL_MS - (Date.now() - lastVisibleTabCaptureStartedAt);
  if (remaining > 0) {
    await new Promise((resolve) => setTimeout(resolve, remaining));
  }
}

async function saveMedia(
  tab: chrome.tabs.Tab,
  kind: "image" | "video",
  mediaUrl: string,
): Promise<void> {
  const tabId = tab.id;
  const loggedMediaUrl = loggableMediaUrl(kind, mediaUrl);
  if (!tabId || !tab.url || !mediaUrl) {
    await log("warn", `Save ${kind} ignored: missing tab or media URL`, {
      tabId,
      pageUrl: tab.url,
      mediaUrl: loggedMediaUrl,
    });
    return;
  }

  await log("info", "Save triggered", { kind, url: tab.url, mediaUrl: loggedMediaUrl });
  await showToast(tabId, "loading", `Saving ${kind}…`);

  let capture: PageCapture;
  try {
    const result = await requestMediaCapture(tabId, tab.url, kind, mediaUrl);
    if (result && "error" in result && result.error) {
      if (result.error_code === "native_connection" && isHostMissing(new Error(result.error))) {
        await log("warn", "Óia app not installed", { error: result.error });
        await notifyHostMissing(tabId);
        return;
      }
      await showBadge(tabId, "error");
      await showToast(tabId, "error", `Couldn't save ${kind}`, result.error);
      await log("error", "Media capture failed", {
        kind,
        mediaUrl: loggedMediaUrl,
        error: result.error,
      });
      return;
    }
    if (isImportedVideoCapture(result)) {
      await presentSaveResponse(tab, result.response, {
        kind: "video",
        item: "video",
        title: result.metadata.title,
        canonicalUrl: result.metadata.canonical_url,
      });
      return;
    }
    capture = result as PageCapture;
  } catch (err) {
    await showBadge(tabId, "error");
    await showToast(
      tabId,
      "error",
      `Couldn't save ${kind}`,
      "The selected media couldn't be read from this page.",
    );
    await log("error", "Could not reach content script for media capture", {
      kind,
      mediaUrl: loggedMediaUrl,
      error: err,
    });
    return;
  }

  await saveCapture(tab, capture);
}

async function saveQuote(tab: chrome.tabs.Tab, text: string): Promise<void> {
  const tabId = tab.id;
  if (!tabId || !tab.url || !text.trim()) {
    await log("warn", "Save quote ignored: missing tab, page URL, or selected text", {
      tabId,
      pageUrl: tab.url,
    });
    return;
  }

  await log("info", "Save triggered", { kind: "quote", url: tab.url });
  await showToast(tabId, "loading", "Saving quote…");

  let capture: PageCapture;
  try {
    const result = await requestQuoteCapture(tabId, tab.url, text);
    if (result && "error" in result && result.error) {
      await showBadge(tabId, "error");
      await showToast(tabId, "error", "Couldn't save quote", result.error);
      await log("error", "Quote capture failed", { url: tab.url, error: result.error });
      return;
    }
    capture = result as PageCapture;
  } catch (err) {
    await showBadge(tabId, "error");
    await showToast(
      tabId,
      "error",
      "Couldn't save quote",
      "The selected text couldn't be read from this page.",
    );
    await log("error", "Could not reach content script for quote capture", {
      url: tab.url,
      error: err,
    });
    return;
  }

  await saveCapture(tab, capture);
}

async function saveCapture(
  tab: chrome.tabs.Tab,
  capture: PageCapture,
  itemOverride?: string,
): Promise<void> {
  const tabId = tab.id!;
  const item =
    itemOverride ?? (capture.metadata.kind === "article" ? "page" : capture.metadata.kind);
  const images = await completeCaptureImages(capture);

  // A standalone image is only a successful save when the selected asset will
  // actually be written into the reading's assets folder. Continuing with an
  // empty (or size-capped) image list would leave a remote-only card while the
  // UI claims it was saved for offline use.
  if (
    capture.metadata.kind === "image" &&
    (!capture.metadata.media_url ||
      !images.some((image) => image.url === capture.metadata.media_url))
  ) {
    await showBadge(tabId, "error");
    await showToast(
      tabId,
      "error",
      `Couldn't save ${item}`,
      "The selected image couldn't be copied into your local library.",
    );
    await log("error", "Selected image bytes were unavailable", {
      url: tab.url,
      mediaUrl: capture.metadata.media_url,
    });
    return;
  }

  // Every successful video save must have completed the acknowledged streaming
  // import before reaching this ordinary JSON save path. Reject any malformed
  // or stale content-script response instead of claiming a poster-only save.
  if (capture.metadata.kind === "video") {
    await showBadge(tabId, "error");
    await showToast(
      tabId,
      "error",
      "Couldn't save video",
      "The selected video couldn't be copied into your local library.",
    );
    await log("error", "Video capture bypassed the required streaming import", {
      url: tab.url,
      mediaUrl: capture.metadata.media_url
        ? loggableMediaUrl("video", capture.metadata.media_url)
        : undefined,
    });
    return;
  }

  const request: SaveRequest = {
    protocol_version: PROTOCOL_VERSION,
    action: "save",
    metadata: capture.metadata,
    markdown: capture.markdown,
    images,
    preview_url: capture.preview_url,
    favicon_url: capture.favicon_url,
  };

  await persistSave(tab, request, feedbackFor(capture, item));
}

/**
 * Retry cross-origin assets in the privileged worker and put the small set of
 * semantic page assets first so a large article cannot crowd its social image
 * or favicon out of the native-message byte budget.
 */
async function completeCaptureImages(capture: PageCapture): Promise<ImageData[]> {
  const fallback = await fetchImages(capture.unresolved);
  const merged = deduplicateImages([...capture.images, ...fallback.images]);
  const previewCandidates = capture.preview_candidates ?? [];
  const faviconCandidates = capture.favicon_candidates ?? [];
  const selectedPreview = previewCandidates.find((url) =>
    merged.some((image) => image.url === url),
  );
  const selectedFavicon = faviconCandidates.find((url) =>
    merged.some((image) => image.url === url),
  );
  const roles = [
    ...new Set([selectedPreview, selectedFavicon].filter((url): url is string => Boolean(url))),
  ];
  const roleCandidates = new Set([...previewCandidates, ...faviconCandidates]);
  const contentImages = new Set(capture.content_image_urls ?? []);
  const kept = merged.filter(
    (image) =>
      !roleCandidates.has(image.url) || roles.includes(image.url) || contentImages.has(image.url),
  );
  const prioritized = [
    ...roles.flatMap((url) => kept.filter((image) => image.url === url)),
    ...kept.filter((image) => !roles.includes(image.url)),
  ];
  const images = capTotalBytes(prioritized, MAX_TOTAL_IMAGE_BYTES);
  capture.preview_url = previewCandidates.find((url) => images.some((image) => image.url === url));
  capture.favicon_url = faviconCandidates.find((url) => images.some((image) => image.url === url));

  await log("info", "Captured images", {
    fromPage: capture.images.length,
    fromBackground: fallback.images.length,
    sent: images.length,
    requested: capture.images.length + capture.unresolved.length,
    preview: Boolean(
      capture.preview_url && images.some((image) => image.url === capture.preview_url),
    ),
    favicon: Boolean(
      capture.favicon_url && images.some((image) => image.url === capture.favicon_url),
    ),
  });
  return images;
}

function addTabFaviconCandidate(capture: PageCapture, value: string | undefined): void {
  if (!value) return;

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return;
  }
  if ((url.protocol !== "http:" && url.protocol !== "https:") || url.username || url.password) {
    return;
  }

  capture.favicon_candidates = [
    url.href,
    ...(capture.favicon_candidates ?? []).filter((candidate) => candidate !== url.href),
  ];
  if (
    !capture.images.some((image) => image.url === url.href) &&
    !capture.unresolved.includes(url.href)
  ) {
    capture.unresolved.unshift(url.href);
  }
}

function deduplicateImages(images: ImageData[]): ImageData[] {
  const seen = new Set<string>();
  return images.filter((image) => {
    if (seen.has(image.url)) return false;
    seen.add(image.url);
    return true;
  });
}

type NativeSaveRequest = SaveRequest | SaveLinkRequest;

interface SaveFeedback {
  kind: SaveKind;
  item: string;
  title: string;
  canonicalUrl: string;
  mediaUrl?: string;
}

function feedbackFor(capture: PageCapture, itemOverride?: string): SaveFeedback {
  return {
    kind: capture.metadata.kind,
    item: itemOverride ?? (capture.metadata.kind === "article" ? "page" : capture.metadata.kind),
    title: capture.metadata.title,
    canonicalUrl: capture.metadata.canonical_url,
    mediaUrl: capture.metadata.media_url,
  };
}

async function persistSave(
  tab: chrome.tabs.Tab,
  request: NativeSaveRequest,
  feedback: SaveFeedback,
): Promise<void> {
  const tabId = tab.id!;

  let response: SaveResponse;
  try {
    response = await sendNativeMessage<NativeSaveRequest, SaveResponse>(request);
  } catch (err) {
    if (err instanceof Error && isHostMissing(err)) {
      await log("warn", "Óia app not installed", { error: err });
      await notifyHostMissing(tabId);
    } else {
      await showBadge(tabId, "error");
      await showToast(
        tabId,
        "error",
        `Couldn't save ${feedback.item}`,
        "The Óia app returned an error.",
      );
      await log("error", "Óia app error", { url: tab.url, error: err });
    }
    return;
  }

  await presentSaveResponse(tab, response, feedback);
}

async function presentSaveResponse(
  tab: chrome.tabs.Tab,
  response: SaveResponse,
  feedback: SaveFeedback,
): Promise<void> {
  const tabId = tab.id!;
  if (response.ok) {
    await showToast(tabId, "ok", "Saved to Óia", feedback.title);
    if (feedback.kind === "article") {
      markSaved(tabId, tab.url!, feedback.canonicalUrl);
    }
    await log("info", "Saved", {
      kind: feedback.kind,
      url: tab.url,
      mediaUrl: feedback.mediaUrl,
      title: feedback.title,
    });
  } else if (response.error === "duplicate") {
    await showToast(tabId, "ok", "Already in Óia", feedback.title);
    if (feedback.kind === "article") {
      markSaved(tabId, tab.url!, feedback.canonicalUrl);
    }
    await log("info", "Already saved (duplicate)", { url: tab.url });
  } else {
    await showBadge(tabId, "error");
    await showToast(
      tabId,
      "error",
      `Couldn't save ${feedback.item}`,
      response.message || response.error,
    );
    await log("error", "Save failed", {
      url: tab.url,
      error: response.error,
      message: response.message,
    });
  }
}

function isImportedVideoCapture(
  capture: StandaloneMediaCaptureResponse,
): capture is ImportedVideoCapture {
  return "video_import" in capture && capture.video_import === true;
}

/** Update the cache and flip the icon to "saved" immediately after a save. */
function markSaved(tabId: number, tabUrl: string, canonicalUrl: string): void {
  savedCache.set(normalizeUrl(tabUrl), true);
  savedCache.set(normalizeUrl(canonicalUrl), true);
  void setIcon("saved", tabId);
}

// ── Triggers ──────────────────────────────────────────────────────────────────

async function handleToolbarSave(kind: ToolbarSaveKind, tabId: number): Promise<void> {
  let tab: chrome.tabs.Tab | undefined;
  try {
    tab = await chrome.tabs.get(tabId);
  } catch (err) {
    await log("error", "Could not find the toolbar's source tab", { kind, tabId, error: err });
    return;
  }
  if (!tab) {
    await log("warn", "Toolbar save ignored: source tab no longer exists", { kind, tabId });
    return;
  }

  if (kind === "article") {
    await savePage(tab);
  } else if (kind === "link") {
    await saveLink(tab);
  } else {
    await saveScreenshot(tab);
  }
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== CONTEXT_MENU_ID || !tab) return;

  const target = contextCaptureTarget(info);
  if (target.kind === "article") {
    void savePage(tab);
  } else if (target.kind === "quote") {
    void saveQuote(tab, target.text);
  } else if (target.mediaUrl) {
    void saveMedia(tab, target.kind, target.mediaUrl);
  }
});

chrome.commands.onCommand.addListener((command) => {
  if (command === "save-page") {
    chrome.tabs.query({ active: true, currentWindow: true }, ([tab]) => {
      if (tab) void savePage(tab);
    });
  }
});

// A document-scoped blob URL can only be read or recorded while its owning tab
// is alive. The content script streams it over this long-lived runtime port;
// the worker owns the one matching native port and serializes every
// request/acknowledgement.
chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== VIDEO_IMPORT_PORT_NAME) return;
  relayVideoImportPort(
    port as unknown as RelayPort,
    () => chrome.runtime.connectNative(HOST_ID) as unknown as RelayPort,
    () => chrome.runtime.lastError?.message,
  );
});

// ── Host-missing notification ─────────────────────────────────────────────────

async function notifyHostMissing(tabId: number): Promise<void> {
  await showBadge(tabId, "error");
  await showToast(
    tabId,
    "error",
    "Óia isn't installed",
    "You need the Óia app to save items to your library.",
    { label: "Get Óia", command: "open-install" },
  );
  // A desktop notification is a fallback for pages where no toast can render
  // (chrome:// pages, the web store, PDFs — the content script can't run there).
  try {
    await chrome.notifications.create(NOTIF_HOST_MISSING, {
      type: "basic",
      iconUrl: chrome.runtime.getURL("icons/icon-128.png"),
      title: "Óia isn't installed",
      message: "You need the Óia app to save items. Click here to learn how to install it.",
      requireInteraction: true,
    });
  } catch (err) {
    // A failed notification must never bubble up as an unhandled rejection; the
    // toast and badge above already convey the problem.
    await log("error", "Could not show host-missing notification", { error: err });
  }
}

function openInstallGuide(): void {
  void chrome.tabs.create({ url: chrome.runtime.getURL("install.html") });
}

chrome.notifications.onClicked.addListener((id) => {
  if (id === NOTIF_HOST_MISSING) {
    openInstallGuide();
    void chrome.notifications.clear(id);
  }
});

// The "How to install" button on the in-page host-missing toast; the content
// script can't open an extension page, so it asks the worker to.
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (isPreparePageVideoBridgeMessage(msg)) {
    const tabId = _sender.tab?.id;
    if (tabId === undefined) {
      sendResponse({ ok: false, message: "The video page could not be identified." });
      return;
    }

    const target: chrome.scripting.InjectionTarget =
      _sender.frameId === undefined ? { tabId } : { tabId, frameIds: [_sender.frameId] };
    void chrome.scripting
      .executeScript({ target, world: "MAIN", files: ["dist/page-video-main.js"] })
      .then(
        () => sendResponse({ ok: true }),
        (error: unknown) => {
          void log("error", "Could not prepare the page video bridge", {
            action: PREPARE_PAGE_VIDEO_BRIDGE,
            error,
          });
          sendResponse({
            ok: false,
            message: error instanceof Error ? error.message : String(error),
          });
        },
      );
    return true;
  }

  if (isToolbarSaveMessage(msg)) {
    void handleToolbarSave(msg.kind, msg.tabId).then(
      () => sendResponse({ accepted: true }),
      (error) => {
        void log("error", "Toolbar save failed unexpectedly", { kind: msg.kind, error });
        sendResponse({ accepted: false });
      },
    );
    // Keep the popup's message port (and therefore this MV3 worker) alive until
    // capture and native persistence finish. The popup closes on the response.
    return true;
  }

  if (msg?.action === "toast-cta" && msg.command === "open-install") {
    openInstallGuide();
  }
});

// ── Helpers ───────────────────────────────────────────────────────────────────

function sendNativeMessage<TReq, TRes>(message: TReq): Promise<TRes> {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendNativeMessage(HOST_ID, message as object, (response: TRes) => {
      if (chrome.runtime.lastError) {
        reject(new Error(chrome.runtime.lastError.message));
      } else {
        resolve(response);
      }
    });
  });
}

async function showToast(
  tabId: number,
  status: ToastMessage["status"],
  title: string,
  detail?: string,
  cta?: ToastMessage["cta"],
): Promise<void> {
  const message: ToastMessage = { action: "toast", status, title, detail, cta };
  try {
    await requestContentResponse(tabId, message);
  } catch {
    // Tab closed/navigated or no content script reachable; the badge still conveys status.
  }
}

async function showBadge(tabId: number, status: "ok" | "error"): Promise<void> {
  const text = status === "ok" ? "✓" : "✗";
  const color = status === "ok" ? "#22C55E" : "#EF4444";
  await chrome.action.setBadgeText({ text, tabId });
  await chrome.action.setBadgeBackgroundColor({ color, tabId });
  setTimeout(() => chrome.action.setBadgeText({ text: "", tabId }), 3000);
}

function loggableMediaUrl(kind: "image" | "video", mediaUrl: string): string {
  return kind === "video" && isSessionLocalVideoUrl(mediaUrl)
    ? "[session-local video source]"
    : mediaUrl;
}
