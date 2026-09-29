// SPDX-License-Identifier: MIT

import { extractPage } from "./extraction.js";
import { fetchImages } from "./images.js";
import { extractQuote, extractStandaloneMedia, type StandaloneMediaKind } from "./media.js";
import { extractPageMetadata } from "./page-metadata.js";
import type {
  ImageData,
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

/** An optional action button on a toast. Clicking it messages the background
 *  worker (which alone can open extension pages) and dismisses the toast. */
export interface ToastCta {
  label: string;
  command: "open-install";
}

/** In-page toast request sent by the background worker after a save attempt. */
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
  images: ImageData[];
  unresolved: string[];
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

const screenshotPageCapture = new ScreenshotPageCaptureController(document, window);

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (isScreenshotPageMessage(msg)) {
    void handleScreenshotPageMessage(screenshotPageCapture, msg).then(sendResponse);
    return true;
  }

  if (msg?.action === "extract") {
    void (async () => {
      const result = extractPage(document, window.location.href);
      if (!result) {
        sendResponse({ error: "Could not extract article content from this page." });
        return;
      }
      // Fetch images here first: in the page's context these reuse the browser's
      // cache, so images the browser already loaded need no network request.
      const { images, unresolved } = await fetchImages(
        captureImageUrls(result.preview_candidates, result.favicon_candidates, result.image_urls),
      );
      const capture: PageCapture = {
        metadata: result.metadata,
        markdown: result.markdown,
        images,
        unresolved,
        preview_candidates: result.preview_candidates,
        favicon_candidates: result.favicon_candidates,
        content_image_urls: result.image_urls,
      };
      sendResponse(capture);
    })();
    return true; // keep the message channel open for the async sendResponse
  }

  if (msg?.action === "capture-link") {
    void (async () => {
      const { pageUrl } = msg as CaptureLinkMessage;
      if (!pageUrl) {
        sendResponse({ error: "The current page link could not be read." });
        return;
      }

      const page = extractPageMetadata(document, pageUrl);
      const title = page.title ?? new URL(pageUrl).hostname;
      const { images, unresolved } = await fetchImages(
        captureImageUrls(page.socialImageUrls, page.faviconUrls),
      );
      const capture: PageCapture = {
        metadata: {
          kind: "article",
          url: pageUrl,
          canonical_url: page.canonicalUrl,
          title,
          saved_at: new Date().toISOString(),
          ...(page.author ? { author: page.author } : {}),
          ...(page.site ? { site: page.site } : {}),
          ...(page.themeColor ? { theme_color: page.themeColor } : {}),
          ...(page.lang ? { lang: page.lang } : {}),
          ...(page.excerpt ? { excerpt: page.excerpt } : {}),
        },
        markdown: "",
        images,
        unresolved,
        preview_candidates: page.socialImageUrls,
        favicon_candidates: page.faviconUrls,
      };
      sendResponse(capture);
    })();
    return true;
  }

  if (msg?.action === "capture-media") {
    void (async () => {
      sendResponse(
        await captureStandaloneMediaRequest(document, msg as CaptureMediaMessage, importVideo),
      );
    })();
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
    sendResponse(capture);
  }

  if (msg?.action === "toast") {
    showToast(msg as ToastMessage);
  }
});

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
  const { images, unresolved } = await fetchImages(result.image_urls);
  return {
    metadata: result.metadata,
    markdown: result.markdown,
    images,
    unresolved,
  };
}

const TOAST_HOST_ID = "oia-toast-host";

/**
 * Show or update the toast. If a loading toast is already present and the new
 * status is ok/error, the existing toast transitions in place instead of being
 * replaced. If the loading toast was already dismissed by the user, a fresh
 * toast is created with the result status.
 */
export function showToast({ status, title, detail, cta }: ToastMessage): void {
  const existingHost = document.getElementById(TOAST_HOST_ID) as HTMLElement | null;

  if (status !== "loading" && existingHost?.dataset.status === "loading") {
    existingHost.dataset.status = status;
    updateToast(existingHost, status, title, detail, cta);
    return;
  }

  existingHost?.remove();

  const isLoading = status === "loading";

  const host = document.createElement("div");
  host.id = TOAST_HOST_ID;
  host.dataset.status = status;
  host.style.cssText =
    "all: initial; position: fixed; top: 16px; right: 16px; z-index: 2147483647;";

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
      .toast {
        display: flex; align-items: center; gap: 11px;
        box-sizing: border-box; width: min(340px, calc(100vw - 32px));
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
      .title {
        overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
        font-size: 13px; font-weight: 650; letter-spacing: -0.01em;
      }
      .detail {
        margin-top: 2px; color: var(--muted);
        overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
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
      .cta {
        margin-top: 8px; cursor: pointer;
        background: var(--action); color: var(--action-text); border: none;
        border-radius: 7px; padding: 7px 12px;
        font: 600 12px/1 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Arial, sans-serif;
      }
      .cta:hover { opacity: 0.84; }
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
        <div class="title"></div>
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
  root.querySelector(".close")!.addEventListener("click", () => host.remove());
  renderCta(root, host, cta);

  (document.body ?? document.documentElement).appendChild(host);

  // A toast with an action stays until the user acts on or dismisses it, the
  // way a "needs your attention" prompt should.
  if (!isLoading && !cta) {
    scheduleDismiss(host);
  }
}

/** Add (or clear) the action button and wire its click to the background. */
function renderCta(root: ShadowRoot, host: HTMLElement, cta?: ToastCta): void {
  root.querySelector(".cta")?.remove();
  if (!cta) return;

  const button = document.createElement("button");
  button.className = "cta";
  button.type = "button";
  button.textContent = cta.label;
  button.addEventListener("click", () => {
    // The content script can't open an extension page; the worker does it.
    void chrome.runtime.sendMessage({ action: "toast-cta", command: cta.command });
    host.remove();
  });
  root.querySelector(".text")!.appendChild(button);
}

function updateToast(
  host: HTMLElement,
  status: "ok" | "error",
  title: string,
  detail?: string,
  cta?: ToastCta,
): void {
  const root = host.shadowRoot!;
  const toastEl = root.querySelector<HTMLElement>(".toast")!;
  toastEl.setAttribute("role", status === "error" ? "alert" : "status");
  toastEl.setAttribute("aria-live", status === "error" ? "assertive" : "polite");

  const spinner = root.querySelector(".spinner");
  if (spinner) {
    const badge = document.createElement("span");
    badge.className = `badge badge--${status}`;
    badge.textContent = status === "ok" ? "✓" : "!";
    spinner.replaceWith(badge);
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

  renderCta(root, host, cta);

  // Keep an actionable toast on screen until the user responds to it.
  if (!cta) scheduleDismiss(host);
}

function scheduleDismiss(host: HTMLElement): void {
  const toast = host.shadowRoot!.querySelector(".toast")!;
  const dismiss = () => host.remove();
  setTimeout(() => {
    toast.classList.add("hide");
    toast.addEventListener("animationend", dismiss, { once: true });
    setTimeout(dismiss, 400); // fallback in case animationend doesn't fire
  }, 3200);
}

function captureImageUrls(
  previewUrls: string[],
  faviconUrls: string[],
  contentUrls: string[] = [],
): string[] {
  return [...new Set([...previewUrls, ...faviconUrls, ...contentUrls])];
}
