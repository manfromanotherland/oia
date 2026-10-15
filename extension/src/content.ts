// SPDX-License-Identifier: MIT

import { extractPage } from "./extraction.js";
import { fetchImages } from "./images.js";
import { extractQuote, extractStandaloneMedia, type StandaloneMediaKind } from "./media.js";
import { extractPageMetadata } from "./page-metadata.js";
import type {
  ImageData,
  PageCaptureEvidence,
  SaveRequestMetadata,
  SaveResponse,
  VideoImportMetadata,
} from "./protocol.js";
import {
  handleScreenshotPageMessage,
  isScreenshotPageMessage,
  ScreenshotPageCaptureController,
} from "./screenshot-page.js";
import { importVideo, type VideoImportOptions, VideoImportError } from "./video-import.js";
import { captureSelection, SelectionContextCache } from "./selection-capture.js";
import { isSamePage, PAGE_CHANGED_MESSAGE, PageChangedError } from "./capture-context.js";
import { assertMessageBudget, fitMessageImages, MessageBudgetError } from "./message-budget.js";

export interface CaptureFailure {
  error: string;
  error_code?: string;
}

/** An optional action handled by the background worker. Screenshot offers have
 *  one fixed deadline, including when an existing toast updates in place. */
export type ToastCta =
  | { label: string; command: "open-install" }
  | { label: string; command: "save-screenshot"; expiresAt: number };

export type SaveChoice = "page" | "screenshot" | "cancel";

/** In-page toast request for a pending save or its result. */
export interface ToastMessage {
  action: "toast";
  status: "ok" | "error" | "loading";
  title: string;
  detail?: string;
  cta?: ToastCta;
}

/**
 * Result of extracting a page and capturing its images. Images the content
 * script could read (same-origin or CORS-enabled, served from the browser cache)
 * come back in `images`; the rest are listed in `unresolved` for the background
 * worker to retry with its cross-origin reach.
 */
export interface PageCapture {
  metadata: SaveRequestMetadata;
  markdown: string;
  /** Raw extracted-content measurements interpreted by the shared core. */
  page_content?: PageCaptureEvidence;
  images: ImageData[];
  unresolved: string[];
  /** Asset URLs omitted by a byte limit; the worker must not fetch them again. */
  skipped?: string[];
  /** Social/meta image URL selected as the card preview, when present. */
  preview_url?: string;
  /** Declared (or conventional) page favicon URL, when present. */
  favicon_url?: string;
  /** Ordered social/meta image candidates; the worker picks the first captured one. */
  preview_candidates?: string[];
  /** Ordered favicon candidates; the worker picks the first captured one. */
  favicon_candidates?: string[];
  /** URLs that are part of the visible Markdown body, not only head metadata. */
  content_image_urls?: string[];
}

interface CaptureLinkMessage {
  action: "capture-link";
  pageUrl: string;
}

export interface CaptureMediaMessage {
  action: "capture-media";
  kind: StandaloneMediaKind;
  mediaUrl: string;
  /** The top-level tab URL remains the saved item's source URL. */
  pageUrl: string;
}

export interface ImportedVideoCapture {
  video_import: true;
  metadata: VideoImportMetadata;
  response: SaveResponse;
}

export type StandaloneMediaCaptureResponse =
  | PageCapture
  | ImportedVideoCapture
  | { error: string; error_code?: string };

type VideoImporter = (options: VideoImportOptions) => Promise<{
  metadata: VideoImportMetadata;
  response: SaveResponse;
}>;

interface CaptureQuoteMessage {
  action: "capture-quote";
  text: string;
  pageUrl: string;
}

interface CaptureSelectionMessage {
  action: "capture-selection";
  text: string;
  pageUrl: string;
}

interface ContentRegistration {
  runtime: typeof chrome.runtime.onMessage;
  dispose: () => void;
}

/** One registration per isolated world, including simultaneous fallback injections. */
export function initializeContentScript(): void {
  const scope = globalThis as typeof globalThis & {
    __oiaContentRegistration?: ContentRegistration;
  };
  const runtime = chrome.runtime.onMessage;
  if (scope.__oiaContentRegistration?.runtime === runtime) return;
  scope.__oiaContentRegistration?.dispose();
  const screenshotPageCapture = new ScreenshotPageCaptureController(document, window);
  const selectionContext = new SelectionContextCache(document);
  const onContextMenu = (event: Event) => selectionContext.capture(event.target);
  document.addEventListener("contextmenu", onContextMenu, true);

  const listener: Parameters<typeof runtime.addListener>[0] = (msg, _sender, sendResponse) => {
    if (isScreenshotPageMessage(msg)) {
      void handleScreenshotPageMessage(screenshotPageCapture, msg).then(sendResponse);
      return true;
    }

    if (msg?.action === "extract" || msg?.action === "capture-page") {
      void respondToCapture(async () => {
        const pageUrl = window.location.href;
        const assertCurrentPage = () => {
          if (
            msg.action === "capture-page" &&
            (!isSamePage(msg.pageUrl, pageUrl) || !isSamePage(pageUrl, window.location.href))
          ) {
            throw new PageChangedError();
          }
        };
        assertCurrentPage();
        const result = extractPage(document, pageUrl);
        if (!result) {
          if (msg.action === "capture-page") {
            const capture = await capturePageMetadata(document, pageUrl);
            assertCurrentPage();
            return boundedCapture(capture);
          }
          return { error: "Could not extract article content from this page." };
        }
        // Fetch images here first: in the page's context these reuse the browser's
        // cache, so images the browser already loaded need no network request.
        const { images, unresolved, skipped } = await fetchImages(
          captureImageUrls(result.preview_candidates, result.favicon_candidates, result.image_urls),
        );
        const capture: PageCapture = {
          metadata: result.metadata,
          markdown: result.markdown,
          images,
          unresolved,
          skipped,
          preview_candidates: result.preview_candidates,
          favicon_candidates: result.favicon_candidates,
          content_image_urls: result.image_urls,
          ...(msg.action === "capture-page" ? { page_content: result.page_content } : {}),
        };
        assertCurrentPage();
        return boundedCapture(capture);
      }, sendResponse);
      return true; // keep the message channel open for the async sendResponse
    }

    if (msg?.action === "capture-link") {
      void respondToCapture(async () => {
        const { pageUrl } = msg as CaptureLinkMessage;
        if (!pageUrl) {
          return { error: "The current page link could not be read." };
        }

        return boundedCapture(await capturePageMetadata(document, pageUrl));
      }, sendResponse);
      return true;
    }

    if (msg?.action === "capture-media") {
      void respondToCapture(async () => {
        const capture = await captureStandaloneMediaRequest(
          document,
          msg as CaptureMediaMessage,
          importVideo,
        );
        if ("metadata" in capture && !("video_import" in capture)) return boundedCapture(capture);
        assertMessageBudget(capture);
        return capture;
      }, sendResponse);
      return true;
    }

    if (msg?.action === "capture-quote") {
      const { text, pageUrl } = msg as CaptureQuoteMessage;
      if (!text?.trim() || !pageUrl) {
        sendResponse({ error: "The selected text could not be read." });
        return;
      }

      const result = extractQuote(document, pageUrl, text);
      const capture: PageCapture = {
        metadata: result.metadata,
        markdown: result.markdown,
        images: [],
        unresolved: [],
      };
      void respondToCapture(async () => boundedCapture(capture), sendResponse);
      return true;
    }

    if (msg?.action === "capture-selection") {
      const { text, pageUrl } = msg as CaptureSelectionMessage;
      if (!text?.trim() || !pageUrl) {
        sendResponse({ error: "The selected text could not be read." });
        return;
      }
      const context = selectionContext.forText(text);
      void respondToCapture(
        async () => boundedCapture(await captureSelection(document, pageUrl, text, context)),
        sendResponse,
      );
      return true;
    }

    if (msg?.action === "save-choice") {
      showToast({ ...msg, action: "toast" } as ToastMessage, (kind) => sendResponse({ kind }));
      return true;
    }

    if (msg?.action === "toast") {
      showToast(msg as ToastMessage);
    }
  };
  runtime.addListener(listener);
  scope.__oiaContentRegistration = {
    runtime,
    dispose: () => {
      document.removeEventListener("contextmenu", onContextMenu, true);
      if (pendingChoiceHost) removeToast(pendingChoiceHost);
      try {
        runtime.removeListener?.(listener);
      } catch {
        /* The old runtime may have been invalidated by an update. */
      }
    },
  };
}

initializeContentScript();

function boundedCapture<T extends PageCapture>(capture: T): T {
  // Reserve the URL bookkeeping before fitting: adding the dropped URLs after
  // filling the envelope could otherwise exceed the same limit again.
  const candidate = {
    ...capture,
    skipped: [
      ...new Set([...(capture.skipped ?? []), ...capture.images.map((image) => image.url)]),
    ],
  };
  const result = capture.metadata.kind === "image" ? { ...capture } : fitMessageImages(candidate);
  const retained = new Set(result.images.map((image) => image.url));
  result.skipped = [
    ...new Set([
      ...(capture.skipped ?? []),
      ...capture.images.filter((image) => !retained.has(image.url)).map((image) => image.url),
    ]),
  ];
  if (result.preview_url && !retained.has(result.preview_url)) delete result.preview_url;
  if (result.favicon_url && !retained.has(result.favicon_url)) delete result.favicon_url;
  assertMessageBudget(result);
  return result;
}

async function respondToCapture(
  capture: () => Promise<unknown>,
  sendResponse: (response: unknown) => void,
): Promise<void> {
  try {
    sendResponse(await capture());
  } catch (error) {
    sendResponse({
      error:
        error instanceof PageChangedError
          ? PAGE_CHANGED_MESSAGE
          : error instanceof Error
            ? error.message
            : "The page couldn't be captured.",
      error_code:
        error instanceof PageChangedError
          ? "page_changed"
          : error instanceof MessageBudgetError
            ? "capture_too_large"
            : "capture_failed",
    } satisfies CaptureFailure);
  }
}

/** Preserve live page evidence even when no readable article was extracted. */
async function capturePageMetadata(doc: Document, pageUrl: string): Promise<PageCapture> {
  const page = extractPageMetadata(doc, pageUrl);
  const { images, unresolved, skipped } = await fetchImages(
    captureImageUrls(page.socialImageUrls, page.faviconUrls),
  );
  return {
    metadata: {
      kind: "article",
      url: pageUrl,
      canonical_url: page.canonicalUrl,
      title: page.title ?? new URL(pageUrl).hostname,
      saved_at: new Date().toISOString(),
      ...(page.author ? { author: page.author } : {}),
      ...(page.site ? { site: page.site } : {}),
      ...(page.themeColor ? { theme_color: page.themeColor } : {}),
      ...(page.productHtml ? { product_html: page.productHtml } : {}),
      ...(page.lang ? { lang: page.lang } : {}),
      ...(page.excerpt ? { excerpt: page.excerpt } : {}),
    },
    markdown: "",
    images,
    unresolved,
    skipped,
    preview_candidates: page.socialImageUrls,
    favicon_candidates: page.faviconUrls,
  };
}

/** Handle the public capture-media request. Every video is imported over the
 * streaming port so a successful save always contains a local movie asset;
 * videos never fall through to the poster-only ordinary save path. */
export async function captureStandaloneMediaRequest(
  doc: Document,
  message: CaptureMediaMessage,
  importSelectedVideo: VideoImporter = importVideo,
): Promise<StandaloneMediaCaptureResponse> {
  const { kind, mediaUrl, pageUrl } = message;
  if ((kind !== "image" && kind !== "video") || !mediaUrl || !pageUrl) {
    return { error: "The selected media could not be identified." };
  }

  if (kind === "video") {
    try {
      const imported = await importSelectedVideo({ doc, pageUrl, mediaUrl });
      return { video_import: true, ...imported };
    } catch (error) {
      return {
        error: error instanceof Error ? error.message : "The temporary video could not be saved.",
        ...(error instanceof VideoImportError ? { error_code: error.code } : {}),
      };
    }
  }

  const result = extractStandaloneMedia(doc, pageUrl, kind, mediaUrl);
  // Only images reach the ordinary save path; every video returned above from
  // the local streaming import.
  const { images, unresolved, skipped } = await fetchImages(result.image_urls);
  return {
    metadata: result.metadata,
    markdown: result.markdown,
    images,
    unresolved,
    skipped,
  };
}

const TOAST_HOST_ID = "oia-toast-host";

interface ToastLifecycle {
  timers: Set<ReturnType<typeof setTimeout>>;
  stopAnimationListener?: () => void;
  cancelChoice?: () => void;
}

const toastLifecycles = new WeakMap<HTMLElement, ToastLifecycle>();
let pendingChoiceHost: HTMLElement | undefined;

function clearToastLifecycle(host: HTMLElement): void {
  const lifecycle = toastLifecycles.get(host);
  if (!lifecycle) return;
  for (const timer of lifecycle.timers) clearTimeout(timer);
  lifecycle.stopAnimationListener?.();
  toastLifecycles.delete(host);
  lifecycle.cancelChoice?.();
}

function beginToastLifecycle(host: HTMLElement): ToastLifecycle {
  clearToastLifecycle(host);
  const lifecycle: ToastLifecycle = { timers: new Set() };
  toastLifecycles.set(host, lifecycle);
  return lifecycle;
}

function removeToast(host: HTMLElement): void {
  clearToastLifecycle(host);
  host.remove();
}

function toastTimeout(
  host: HTMLElement,
  lifecycle: ToastLifecycle,
  callback: () => void,
  delay: number,
): void {
  const timer = setTimeout(() => {
    lifecycle.timers.delete(timer);
    if (toastLifecycles.get(host) === lifecycle) callback();
  }, delay);
  lifecycle.timers.add(timer);
}

/**
 * Show or update the toast. If a loading toast is already present and the new
 * status is ok/error, the existing toast transitions in place instead of being
 * replaced. If the loading toast was already dismissed by the user, a fresh
 * toast is created with the result status.
 */
export function showToast(
  { status, title, detail, cta }: ToastMessage,
  onChoice?: (kind: SaveChoice) => void,
): void {
  // A late update from an earlier save must not interrupt the current choice.
  if (pendingChoiceHost && !onChoice) return;
  const existingHost = document.getElementById(TOAST_HOST_ID) as HTMLElement | null;
  if (pendingChoiceHost && pendingChoiceHost !== existingHost) {
    clearToastLifecycle(pendingChoiceHost);
  }

  if (status !== "loading" && existingHost?.dataset.status === "loading") {
    existingHost.dataset.status = status;
    updateToast(existingHost, status, title, detail, cta, onChoice);
    return;
  }

  if (existingHost) removeToast(existingHost);

  const isLoading = status === "loading";

  const host = document.createElement("div");
  host.id = TOAST_HOST_ID;
  host.dataset.status = status;
  host.style.cssText =
    "all: initial; position: fixed; inset: 16px 16px auto auto; margin: 0; padding: 0; border: 0; background: transparent; overflow: visible; z-index: 2147483647;";
  // The top layer keeps the toast above page dialogs and stacking contexts.
  // Older browsers retain the same fixed-position fallback.
  const supportsPopover = typeof host.showPopover === "function";
  if (supportsPopover) host.setAttribute("popover", "manual");
  const lifecycle = beginToastLifecycle(host);

  const root = host.attachShadow({ mode: "open" });
  // Inline the app's eye mark so it works on every page without a web-accessible asset.
  root.innerHTML = `
    <style>
      :host {
        --paper: #fdfcfb;
        --ink: #17181a;
        --muted: #55565a;
        --subtle: #85868b;
        --line: rgb(23 24 26 / 0.12);
        --hover: #eeedec;
        --action: #17181a;
        --action-text: #f7f6f4;
        color-scheme: light dark;
      }
      @media (prefers-color-scheme: dark) {
        :host {
          --paper: #161618;
          --ink: #f1f0ee;
          --muted: #9a9a9e;
          --subtle: #9a9a9e;
          --line: rgb(255 255 255 / 0.15);
          --hover: #242426;
          --action: #e8e9eb;
          --action-text: #17181a;
        }
      }
      @keyframes oia-in { from { opacity: 0; transform: translateY(-6px); } to { opacity: 1; transform: none; } }
      @keyframes oia-out { to { opacity: 0; transform: translateY(-6px); } }
      @keyframes oia-spin { to { transform: rotate(360deg); } }
      @keyframes oia-countdown { to { transform: scaleX(0); } }
      .toast {
        display: flex; align-items: center; gap: 11px;
        box-sizing: border-box; width: min(340px, calc(100vw - 32px));
        max-height: calc(100dvh - 32px); overflow-y: auto;
        min-height: 70px; padding: 12px 11px 12px 13px;
        font: 400 13px/1.4 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Arial, sans-serif;
        color: var(--ink); background: var(--paper);
        border: 1px solid var(--line); border-radius: 13px;
        box-shadow: 0 12px 32px rgb(0 0 0 / 0.13), 0 2px 8px rgb(0 0 0 / 0.06);
        animation: oia-in 180ms ease-out;
      }
      .toast.hide { animation: oia-out 200ms ease-in forwards; }
      .mark {
        position: relative; flex: 0 0 36px; width: 36px; height: 36px;
        display: grid; place-items: center;
        color: #f1f0ee; background: #17181a;
        border: 1px solid rgb(255 255 255 / 0.12);
        border-radius: 9px; box-sizing: border-box;
      }
      .mark svg { display: block; width: 32px; height: 32px; }
      .badge {
        position: absolute; right: -4px; bottom: -4px;
        box-sizing: border-box; width: 17px; height: 17px;
        display: grid; place-items: center; border-radius: 50%;
        border: 2px solid var(--paper); color: #17181a;
        font-size: 10px; font-weight: 800; line-height: 1;
      }
      .badge--ok { background: #ffe066; }
      .badge--error { background: #ff5f57; }
      .spinner {
        position: absolute; right: -4px; bottom: -4px;
        box-sizing: border-box; width: 17px; height: 17px;
        border: 2px solid var(--paper); border-radius: 50%;
        background: var(--paper); display: grid; place-items: center;
      }
      .spinner::after {
        content: ""; box-sizing: border-box; width: 12px; height: 12px;
        border: 2px solid var(--ink); border-top-color: transparent;
        border-radius: 50%; animation: oia-spin 700ms linear infinite;
      }
      .text { min-width: 0; flex: 1; }
      .headline {
        display: flex; align-items: center; justify-content: space-between;
        gap: 12px; min-width: 0;
      }
      .title {
        min-width: 0; flex: 1;
        overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
        font-size: 13px; font-weight: 650; letter-spacing: -0.01em;
      }
      .detail {
        margin-top: 2px; color: var(--muted);
        white-space: normal; overflow-wrap: anywhere;
      }
      .close {
        cursor: pointer; background: transparent; border: 0; padding: 0;
        flex: 0 0 auto; width: 28px; height: 28px; border-radius: 7px;
        display: grid; place-items: center;
        color: var(--subtle);
      }
      .close svg { display: block; width: 16px; height: 16px; }
      .close:hover { color: var(--ink); background: var(--hover); }
      .close:focus-visible, .cta:focus-visible {
        outline: 2px solid var(--ink); outline-offset: 2px;
      }
      .actions { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
      .cta {
        cursor: pointer;
        min-height: 28px;
        background: var(--action); color: var(--action-text); border: none;
        border-radius: 7px; padding: 7px 12px;
        font: 600 12px/1 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Arial, sans-serif;
      }
      .cta:hover { opacity: 0.84; }
      .cta:disabled { cursor: wait; opacity: 0.6; }
      .actions--inline { flex: 0 0 auto; margin: 0; }
      .cta--link {
        min-height: 24px; padding: 0; border-radius: 2px;
        color: var(--muted); background: transparent;
        font-weight: 500; white-space: nowrap;
        text-decoration: underline; text-underline-offset: 2px;
      }
      .countdown {
        height: 2px; margin-top: 8px; overflow: hidden;
        background: var(--line); border-radius: 1px;
      }
      .countdown::after {
        content: ""; display: block; height: 100%; background: var(--ink);
        transform-origin: left;
      }
      @media (prefers-reduced-motion: no-preference) {
        .countdown::after { animation: oia-countdown var(--countdown-duration) linear forwards; }
      }
      @media (prefers-reduced-motion: reduce) {
        .toast, .toast.hide, .spinner::after { animation: none; }
      }
      @media (forced-colors: active) {
        .toast { color: CanvasText; background: Canvas; border-color: CanvasText; box-shadow: none; }
        .mark { color: Canvas; background: CanvasText; border-color: CanvasText; }
        .badge { color: HighlightText; background: Highlight; border-color: Canvas; }
        .close, .detail { color: CanvasText; }
        .close:focus-visible, .cta:focus-visible { outline-color: Highlight; }
        .cta { color: HighlightText; background: Highlight; }
        .cta--link { color: LinkText; background: transparent; }
        .countdown { background: Canvas; outline: 1px solid CanvasText; }
        .countdown::after { background: CanvasText; }
      }
    </style>
    <div class="toast" role="${status === "error" ? "alert" : "status"}" aria-live="${status === "error" ? "assertive" : "polite"}">
      <div class="mark" aria-hidden="true">
        <svg viewBox="0 0 1024 1024" fill="none" aria-hidden="true">
          <path d="M672 513C672 424.634 600.366 353 512 353C423.634 353 352 424.634 352 513C352 601.366 423.634 673 512 673V713C401.543 713 312 623.457 312 513C312 402.543 401.543 313 512 313C622.457 313 712 402.543 712 513C712 623.457 622.457 713 512 713V673C600.366 673 672 601.366 672 513Z" fill="currentColor"/>
          <path d="M512 293C683.874 293 835.229 380.815 923.659 513.934C929.771 523.134 927.267 535.547 918.066 541.659C908.866 547.771 896.453 545.267 890.341 536.066C809.008 413.633 669.916 333 512 333C354.084 333 214.992 413.633 133.659 536.066C127.547 545.267 115.134 547.771 105.934 541.659C96.7331 535.547 94.229 523.134 100.341 513.934C188.771 380.815 340.126 293 512 293Z" fill="currentColor"/>
        </svg>
      ${
        isLoading
          ? '<span class="spinner"></span>'
          : `<span class="badge badge--${status}">${status === "ok" ? "✓" : "!"}</span>`
      }
      </div>
      <div class="text">
        <div class="headline"><div class="title"></div></div>
        ${detail ? '<div class="detail"></div>' : ""}
      </div>
      <button class="close" type="button" aria-label="Dismiss Óia notification">
        <svg viewBox="0 0 20 20" fill="none" aria-hidden="true">
          <path d="M5 5L15 15M15 5L5 15" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
        </svg>
      </button>
    </div>
  `;

  // Page-derived strings go in via textContent, never innerHTML, to avoid injection.
  root.querySelector(".title")!.textContent = title;
  if (detail) root.querySelector(".detail")!.textContent = detail;
  root.querySelector(".close")!.addEventListener("click", () => {
    // Dismissing the notification leaves the pending default save scheduled.
    if (toastLifecycles.get(host)?.cancelChoice) host.remove();
    else removeToast(host);
  });

  (document.body ?? document.documentElement).appendChild(host);
  if (supportsPopover) {
    try {
      host.showPopover();
    } catch {
      host.removeAttribute("popover");
    }
  }
  renderCta(root, host, lifecycle, cta, onChoice);

  // Install prompts persist; screenshot offers use their fixed worker deadline.
  if (!isLoading && !cta) {
    scheduleDismiss(host, lifecycle);
  }
}

/** Add (or clear) the action button and wire its click to the background. */
function renderCta(
  root: ShadowRoot,
  host: HTMLElement,
  lifecycle: ToastLifecycle,
  cta?: ToastCta,
  onChoice?: (kind: SaveChoice) => void,
): void {
  root.querySelector(".actions")?.remove();
  root.querySelector(".countdown")?.remove();
  if (!cta) return;

  const actions = document.createElement("div");
  actions.className = "actions";
  const button = document.createElement("button");
  button.className = "cta";
  button.type = "button";
  button.textContent = cta.label;
  actions.appendChild(button);
  if (cta.command === "save-screenshot") {
    actions.classList.add("actions--inline");
    button.classList.add("cta--link");
    root.querySelector(".headline")!.appendChild(actions);
  } else {
    root.querySelector(".text")!.appendChild(actions);
  }

  if (cta.command === "open-install") {
    button.addEventListener("click", () => {
      // The content script can't open an extension page; the worker does it.
      void chrome.runtime.sendMessage({ action: "toast-cta", command: cta.command });
      removeToast(host);
    });
    return;
  }

  pendingChoiceHost = host;
  let decided = false;
  const choose = (kind: SaveChoice) => {
    if (decided) return;
    decided = true;
    if (pendingChoiceHost === host) pendingChoiceHost = undefined;
    lifecycle.cancelChoice = undefined;
    button.disabled = true;
    for (const timer of lifecycle.timers) clearTimeout(timer);
    lifecycle.timers.clear();
    actions.remove();
    root.querySelector(".countdown")?.remove();
    if (kind === "cancel" && toastLifecycles.get(host) === lifecycle) removeToast(host);
    else if (kind === "screenshot")
      root.querySelector(".title")!.textContent = "Saving screenshot…";
    if (!host.isConnected) clearToastLifecycle(host);
    onChoice?.(kind);
  };
  lifecycle.cancelChoice = () => choose("cancel");
  button.addEventListener("click", () => {
    choose(Date.now() >= cta.expiresAt ? "page" : "screenshot");
  });

  const remaining = cta.expiresAt - Date.now();
  if (remaining <= 0) {
    choose("page");
    return;
  }
  const countdown = document.createElement("div");
  countdown.className = "countdown";
  countdown.setAttribute("aria-hidden", "true");
  countdown.style.setProperty("--countdown-duration", `${remaining}ms`);
  root.querySelector(".text")!.appendChild(countdown);
  toastTimeout(host, lifecycle, () => choose("page"), remaining);
}

function updateToast(
  host: HTMLElement,
  status: "ok" | "error",
  title: string,
  detail?: string,
  cta?: ToastCta,
  onChoice?: (kind: SaveChoice) => void,
): void {
  const lifecycle = beginToastLifecycle(host);
  host.dataset.status = status;
  const root = host.shadowRoot!;
  const toastEl = root.querySelector<HTMLElement>(".toast")!;
  toastEl.setAttribute("role", status === "error" ? "alert" : "status");
  toastEl.setAttribute("aria-live", status === "error" ? "assertive" : "polite");

  const indicator = root.querySelector(".spinner, .badge");
  if (indicator) {
    const badge = document.createElement("span");
    badge.className = `badge badge--${status}`;
    badge.textContent = status === "ok" ? "✓" : "!";
    indicator.replaceWith(badge);
  }

  root.querySelector(".title")!.textContent = title;

  const textEl = root.querySelector(".text")!;
  let detailEl = root.querySelector(".detail");
  if (detail) {
    if (!detailEl) {
      detailEl = document.createElement("div");
      detailEl.className = "detail";
      textEl.appendChild(detailEl);
    }
    detailEl.textContent = detail;
  } else {
    detailEl?.remove();
  }

  renderCta(root, host, lifecycle, cta, onChoice);

  if (!cta) scheduleDismiss(host, lifecycle);
}

function scheduleDismiss(host: HTMLElement, lifecycle: ToastLifecycle): void {
  toastTimeout(host, lifecycle, () => dismissToast(host, lifecycle), 3200);
}

function dismissToast(host: HTMLElement, lifecycle: ToastLifecycle): void {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    removeToast(host);
    return;
  }
  const toast = host.shadowRoot!.querySelector(".toast")!;
  const dismiss = (event: Event) => {
    if (event.target === toast) removeToast(host);
  };
  toast.classList.add("hide");
  toast.addEventListener("animationend", dismiss);
  lifecycle.stopAnimationListener = () => toast.removeEventListener("animationend", dismiss);
  toastTimeout(host, lifecycle, () => removeToast(host), 400);
}

function captureImageUrls(
  previewUrls: string[],
  faviconUrls: string[],
  contentUrls: string[] = [],
): string[] {
  return [...new Set([...previewUrls, ...faviconUrls, ...contentUrls])];
}
