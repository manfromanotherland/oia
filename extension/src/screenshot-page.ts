// SPDX-License-Identifier: MIT

import type { ScreenshotPageState } from "./full-page-screenshot.js";

export const SCREENSHOT_PAGE_BEGIN_ACTION = "screenshot-capture-begin";
export const SCREENSHOT_PAGE_SCROLL_ACTION = "screenshot-capture-scroll";
export const SCREENSHOT_PAGE_MEASURE_ACTION = "screenshot-capture-measure";
export const SCREENSHOT_PAGE_FINISH_ACTION = "screenshot-capture-finish";

const OIA_TOAST_HOST_ID = "oia-toast-host";
const CAPTURE_STYLE_ATTRIBUTE = "data-oia-screenshot-capture";
const CAPTURE_SESSION_TIMEOUT_MS = 120_000;
const ANIMATION_FRAME_FALLBACK_MS = 100;
const CONTENT_SETTLE_DELAY_MS = 75;
const IMAGE_DECODE_TIMEOUT_MS = 250;
const SCROLL_RANGE_TOLERANCE = 0.5;
const SCROLLPORT_GEOMETRY_TOLERANCE = 0.1;
const DOMINANT_SCROLL_SURFACE_WIDTH_RATIO = 0.5;
const DOMINANT_SCROLL_SURFACE_HEIGHT_RATIO = 0.6;

export interface ScreenshotPageBeginMessage {
  action: typeof SCREENSHOT_PAGE_BEGIN_ACTION;
}

export interface ScreenshotPageScrollMessage {
  action: typeof SCREENSHOT_PAGE_SCROLL_ACTION;
  sessionId: string;
  scrollY: number;
  hideFixed: boolean;
}

export interface ScreenshotPageFinishMessage {
  action: typeof SCREENSHOT_PAGE_FINISH_ACTION;
  sessionId: string;
}

export interface ScreenshotPageMeasureMessage {
  action: typeof SCREENSHOT_PAGE_MEASURE_ACTION;
  sessionId: string;
}

export type ScreenshotPageMessage =
  | ScreenshotPageBeginMessage
  | ScreenshotPageScrollMessage
  | ScreenshotPageMeasureMessage
  | ScreenshotPageFinishMessage;

export interface ScreenshotPageFinished {
  ok: true;
}

export interface ScreenshotPageError {
  error: string;
}

export type ScreenshotPageResponse =
  | ScreenshotPageState
  | ScreenshotPageFinished
  | ScreenshotPageError;

interface ScreenshotPageWindow {
  readonly innerWidth: number;
  readonly innerHeight: number;
  readonly scrollX: number;
  readonly scrollY: number;
  readonly location: Pick<Location, "href">;
  scrollTo(options: ScrollToOptions): void;
  getComputedStyle(element: Element): CSSStyleDeclaration;
  requestAnimationFrame(callback: FrameRequestCallback): number;
}

interface HiddenElement {
  element: Element & ElementCSSInlineStyle;
  visibility: string;
  priority: string;
}

interface CaptureSession {
  id: string;
  originalScrollX: number;
  originalScrollY: number;
  scrollElement?: HTMLElement;
  style: HTMLStyleElement;
  hiddenElements: HiddenElement[];
  deferredElements: HiddenElement[];
  hidden: Set<Element>;
  deferredPositionedElements: Set<Element>;
  capturedPositionedElements: Set<Element>;
  restoreTimer?: ReturnType<typeof globalThis.setTimeout>;
}

export interface ScreenshotPageCaptureOptions {
  createSessionId?: () => string;
  settle?: () => Promise<void>;
}

interface ScreenshotCrypto {
  randomUUID?: () => string;
  getRandomValues(array: Uint8Array): Uint8Array;
}

/** Content scripts on HTTP pages may not expose the secure-context randomUUID API. */
export function createScreenshotSessionId(source: ScreenshotCrypto = crypto): string {
  try {
    if (typeof source.randomUUID === "function") return source.randomUUID();
  } catch {
    // getRandomValues remains available in non-secure contexts.
  }

  const bytes = new Uint8Array(16);
  source.getRandomValues(bytes);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/**
 * Owns the temporary page changes needed while the worker scrolls and captures
 * each viewport. Keeping this state in the content script lets every exit path
 * put the live page back exactly as the user left it.
 */
export class ScreenshotPageCaptureController {
  private session: CaptureSession | undefined;
  private readonly createSessionId: () => string;
  private readonly settle: () => Promise<void>;

  constructor(
    private readonly doc: Document,
    private readonly win: ScreenshotPageWindow,
    options: ScreenshotPageCaptureOptions = {},
  ) {
    this.createSessionId = options.createSessionId ?? createScreenshotSessionId;
    this.settle = options.settle ?? (() => settlePage(this.doc, this.win));
  }

  async begin(): Promise<ScreenshotPageState> {
    if (this.session) await this.restore(this.session);

    const scrollElement = viewportScrollElement(this.doc, this.win);

    const style = this.doc.createElement("style");
    style.setAttribute(CAPTURE_STYLE_ATTRIBUTE, "");
    style.textContent = `
      :root, body, * {
        scroll-behavior: auto !important;
        scroll-snap-type: none !important;
      }
      *, *::before, *::after {
        animation-play-state: paused !important;
        transition: none !important;
      }
    `;

    const session: CaptureSession = {
      id: this.createSessionId(),
      originalScrollX: finiteNumber(scrollElement?.scrollLeft ?? this.win.scrollX),
      originalScrollY: finiteNumber(scrollElement?.scrollTop ?? this.win.scrollY),
      ...(scrollElement ? { scrollElement } : {}),
      style,
      hiddenElements: [],
      deferredElements: [],
      hidden: new Set(),
      deferredPositionedElements: new Set(),
      capturedPositionedElements: new Set(),
    };
    this.session = session;
    session.restoreTimer = globalThis.setTimeout(() => {
      if (this.session !== session) return;
      void this.restore(session).catch(() => undefined);
    }, CAPTURE_SESSION_TIMEOUT_MS);
    try {
      (this.doc.head ?? this.doc.documentElement).append(style);
      await this.settle();
      this.requireSession(session.id);
      return this.state(session);
    } catch (error) {
      if (this.session === session) {
        try {
          await this.restore(session);
        } catch {
          // Preserve the error that interrupted setup; restoration is best-effort.
        }
      }
      throw error;
    }
  }

  async scrollTo(
    sessionId: string,
    scrollY: number,
    hideFixed: boolean,
  ): Promise<ScreenshotPageState> {
    const session = this.requireSession(sessionId);
    if (!Number.isFinite(scrollY)) throw new Error("The screenshot scroll position is invalid.");

    restoreHiddenElements(session.deferredElements);
    this.scrollToPosition(session, session.originalScrollX, Math.max(0, scrollY));
    await this.settle();
    this.requireSession(sessionId);

    const state = this.state(session);
    const hasLaterTile =
      state.scrollY + state.viewportHeight < state.documentHeight - SCROLL_RANGE_TOLERANCE;
    const hiddenElement = this.trackAndHideRepeatedPositionedElements(
      session,
      hideFixed,
      hasLaterTile,
    );
    if (hiddenElement) await this.settle();

    this.requireSession(sessionId);
    return this.state(session);
  }

  async finish(sessionId: string): Promise<void> {
    await this.restore(this.requireSession(sessionId));
  }

  measure(sessionId: string): ScreenshotPageState {
    return this.state(this.requireSession(sessionId));
  }

  private requireSession(sessionId: string): CaptureSession {
    if (!sessionId || this.session?.id !== sessionId) {
      throw new Error("The screenshot capture session is no longer active.");
    }
    return this.session;
  }

  private state(session: CaptureSession): ScreenshotPageState {
    if (session.scrollElement) {
      assertActiveViewportScrollElement(session.scrollElement, this.doc, this.win);
    }
    const scrollY = session.scrollElement
      ? finiteNumber(session.scrollElement.scrollTop)
      : finiteNumber(this.win.scrollY);
    const pageHeight = session.scrollElement
      ? Math.max(positiveNumber(this.win.innerHeight), session.scrollElement.scrollHeight)
      : documentHeight(this.doc, this.win.innerHeight);
    return {
      sessionId: session.id,
      url: this.win.location.href,
      viewportWidth: positiveNumber(this.win.innerWidth),
      viewportHeight: positiveNumber(this.win.innerHeight),
      documentHeight: pageHeight,
      scrollY,
    };
  }

  private scrollToPosition(session: CaptureSession, left: number, top: number): void {
    if (session.scrollElement) {
      session.scrollElement.scrollLeft = left;
      session.scrollElement.scrollTop = top;
      return;
    }
    this.win.scrollTo({ left, top, behavior: "auto" });
  }

  /**
   * Keep fixed/sticky UI in the first tile where it appears, then hide it if
   * it remains visible in a later tile. This avoids duplicate headers without
   * deleting section headers that start below the first viewport.
   */
  private trackAndHideRepeatedPositionedElements(
    session: CaptureSession,
    hideRepeated: boolean,
    hasLaterTile: boolean,
  ): boolean {
    let hidElement = false;
    for (const element of Array.from(this.doc.querySelectorAll("*"))) {
      if (
        session.hidden.has(element) ||
        (session.scrollElement &&
          (element === session.scrollElement || element.contains(session.scrollElement))) ||
        !hasInlineStyle(element) ||
        !isInViewport(element, this.win)
      ) {
        continue;
      }

      const position = this.win.getComputedStyle(element).position;
      const isOiaToast = element.id === OIA_TOAST_HOST_ID;
      const isStationaryScrollSibling = Boolean(
        session.scrollElement &&
        !session.scrollElement.contains(element) &&
        !element.contains(session.scrollElement) &&
        element.parentElement?.contains(session.scrollElement),
      );
      if (
        !isOiaToast &&
        !isStationaryScrollSibling &&
        position !== "fixed" &&
        position !== "sticky" &&
        position !== "-webkit-sticky"
      ) {
        continue;
      }

      if (!isOiaToast && (!hideRepeated || !session.capturedPositionedElements.has(element))) {
        if (
          isStationaryScrollSibling ||
          position === "fixed" ||
          isFullyVisibleVertically(element, this.win) ||
          session.deferredPositionedElements.has(element) ||
          !hasLaterTile
        ) {
          session.capturedPositionedElements.add(element);
        } else {
          session.deferredPositionedElements.add(element);
          session.deferredElements.push(hideElement(element));
          hidElement = true;
        }
        continue;
      }

      session.hidden.add(element);
      session.hiddenElements.push(hideElement(element));
      hidElement = true;
    }
    return hidElement;
  }

  private async restore(session: CaptureSession): Promise<void> {
    if (this.session === session) this.session = undefined;
    if (session.restoreTimer !== undefined) globalThis.clearTimeout(session.restoreTimer);

    restoreHiddenElements(session.hiddenElements);
    restoreHiddenElements(session.deferredElements);
    session.hidden.clear();
    session.deferredPositionedElements.clear();
    session.capturedPositionedElements.clear();
    session.style.remove();

    this.scrollToPosition(session, session.originalScrollX, session.originalScrollY);
    await this.settle();
  }
}

export function isScreenshotPageMessage(value: unknown): value is ScreenshotPageMessage {
  if (!value || typeof value !== "object") return false;
  const message = value as Partial<ScreenshotPageMessage>;

  if (message.action === SCREENSHOT_PAGE_BEGIN_ACTION) return true;
  if (
    message.action === SCREENSHOT_PAGE_MEASURE_ACTION ||
    message.action === SCREENSHOT_PAGE_FINISH_ACTION
  ) {
    return typeof message.sessionId === "string" && Boolean(message.sessionId);
  }
  return (
    message.action === SCREENSHOT_PAGE_SCROLL_ACTION &&
    typeof message.sessionId === "string" &&
    Boolean(message.sessionId) &&
    typeof message.scrollY === "number" &&
    Number.isFinite(message.scrollY) &&
    typeof message.hideFixed === "boolean"
  );
}

export async function handleScreenshotPageMessage(
  controller: ScreenshotPageCaptureController,
  message: ScreenshotPageMessage,
): Promise<ScreenshotPageResponse> {
  try {
    if (message.action === SCREENSHOT_PAGE_BEGIN_ACTION) return await controller.begin();
    if (message.action === SCREENSHOT_PAGE_SCROLL_ACTION) {
      return await controller.scrollTo(message.sessionId, message.scrollY, message.hideFixed);
    }
    if (message.action === SCREENSHOT_PAGE_MEASURE_ACTION) {
      return controller.measure(message.sessionId);
    }
    await controller.finish(message.sessionId);
    return { ok: true };
  } catch (error) {
    return {
      error: error instanceof Error ? error.message : "The screenshot capture could not continue.",
    };
  }
}

async function settlePage(doc: Document, win: ScreenshotPageWindow): Promise<void> {
  await nextAnimationFrame(win);
  await nextAnimationFrame(win);
  await delay(CONTENT_SETTLE_DELAY_MS);
  await waitForVisibleImages(doc, win);
  await nextAnimationFrame(win);
}

async function waitForVisibleImages(doc: Document, win: ScreenshotPageWindow): Promise<void> {
  const pending = Array.from(doc.querySelectorAll<HTMLImageElement>("img"))
    .filter((image) => Boolean(image.currentSrc || image.src) && isInViewport(image, win))
    .map((image) => image.decode().catch(() => undefined));
  if (!pending.length) return;

  await Promise.race([Promise.all(pending), delay(IMAGE_DECODE_TIMEOUT_MS)]);
}

function nextAnimationFrame(win: ScreenshotPageWindow): Promise<void> {
  return new Promise((resolve) => {
    let resolved = false;
    const finish = () => {
      if (resolved) return;
      resolved = true;
      globalThis.clearTimeout(fallback);
      resolve();
    };
    const fallback = globalThis.setTimeout(finish, ANIMATION_FRAME_FALLBACK_MS);
    try {
      win.requestAnimationFrame(finish);
    } catch {
      finish();
    }
  });
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => globalThis.setTimeout(resolve, milliseconds));
}

function documentHeight(doc: Document, viewportHeight: number): number {
  const root = doc.documentElement;
  const body = doc.body;
  return Math.max(
    positiveNumber(viewportHeight),
    root?.scrollHeight ?? 0,
    root?.offsetHeight ?? 0,
    root?.clientHeight ?? 0,
    body?.scrollHeight ?? 0,
    body?.offsetHeight ?? 0,
    body?.clientHeight ?? 0,
  );
}

/**
 * Some app-style pages keep the document fixed and put their entire viewport
 * in one scrolling surface. Treat that surface like the page scroller when it
 * exactly covers the viewport; smaller panels need a different crop geometry.
 */
function viewportScrollElement(doc: Document, win: ScreenshotPageWindow): HTMLElement | undefined {
  const documentScrolls =
    documentHeight(doc, win.innerHeight) > win.innerHeight + SCROLL_RANGE_TOLERANCE;
  const fullViewportCandidates: HTMLElement[] = [];
  let unsupportedDominantSurface = false;
  let hiddenOverflowSurface = false;

  for (const element of Array.from(doc.querySelectorAll<HTMLElement>("*"))) {
    if (element === doc.documentElement || element === doc.body) continue;
    const bounds = element.getBoundingClientRect();
    const fillsViewport = isFullViewport(bounds, win);
    if (element.tagName === "IFRAME" && fillsViewport) unsupportedDominantSurface = true;
    if (element.scrollHeight <= element.clientHeight + SCROLL_RANGE_TOLERANCE) continue;
    if (!isDominantViewportSurface(bounds, win)) continue;

    const style = win.getComputedStyle(element);
    if (!hasScrollableOverflow(style)) {
      // Cropped images and visible overflow do not compete with the page scroller.
      // Hidden overflow can still be script-scrolled when it is the only surface.
      if (style.overflowY === "hidden") hiddenOverflowSurface = true;
      continue;
    }

    const hasMatchingScrollport =
      fillsViewport &&
      Math.abs(element.clientHeight - win.innerHeight) <= SCROLLPORT_GEOMETRY_TOLERANCE &&
      Math.abs(bounds.bottom - bounds.top - element.clientHeight) <= SCROLLPORT_GEOMETRY_TOLERANCE;
    if (hasMatchingScrollport && isPaintedAndUntransformed(element, doc, win)) {
      fullViewportCandidates.push(element);
    } else {
      unsupportedDominantSurface = true;
    }
  }

  if (fullViewportCandidates.length > 1) {
    throw new Error("This page has more than one full-page scrolling surface.");
  }
  if (
    (documentScrolls && fullViewportCandidates.length > 0) ||
    (unsupportedDominantSurface && (documentScrolls || fullViewportCandidates.length > 0))
  ) {
    throw new Error("This page has competing scrolling surfaces that cannot be captured safely.");
  }
  if (documentScrolls) return undefined;
  if (fullViewportCandidates.length === 1) return fullViewportCandidates[0];
  if (unsupportedDominantSurface || hiddenOverflowSurface) {
    throw new Error("This page's scrolling layout cannot be captured as one long screenshot.");
  }
  return undefined;
}

function assertActiveViewportScrollElement(
  element: HTMLElement,
  doc: Document,
  win: ScreenshotPageWindow,
): void {
  const bounds = element.getBoundingClientRect();
  if (
    !element.isConnected ||
    !doc.documentElement.contains(element) ||
    element.scrollHeight <= element.clientHeight + SCROLL_RANGE_TOLERANCE ||
    !isFullViewport(bounds, win) ||
    Math.abs(element.clientHeight - win.innerHeight) > SCROLLPORT_GEOMETRY_TOLERANCE ||
    Math.abs(bounds.bottom - bounds.top - element.clientHeight) > SCROLLPORT_GEOMETRY_TOLERANCE ||
    !hasScrollableOverflow(win.getComputedStyle(element)) ||
    !isPaintedAndUntransformed(element, doc, win)
  ) {
    throw new Error("The page's scrolling surface changed during the screenshot.");
  }
}

function isFullViewport(bounds: DOMRect, win: ScreenshotPageWindow): boolean {
  return (
    Math.abs(bounds.top) <= SCROLLPORT_GEOMETRY_TOLERANCE &&
    Math.abs(bounds.left) <= SCROLLPORT_GEOMETRY_TOLERANCE &&
    Math.abs(bounds.bottom - win.innerHeight) <= SCROLLPORT_GEOMETRY_TOLERANCE &&
    Math.abs(bounds.right - win.innerWidth) <= SCROLLPORT_GEOMETRY_TOLERANCE
  );
}

function isDominantViewportSurface(bounds: DOMRect, win: ScreenshotPageWindow): boolean {
  const visibleWidth = Math.max(
    0,
    Math.min(bounds.right, win.innerWidth) - Math.max(bounds.left, 0),
  );
  const visibleHeight = Math.max(
    0,
    Math.min(bounds.bottom, win.innerHeight) - Math.max(bounds.top, 0),
  );
  return (
    visibleWidth / positiveNumber(win.innerWidth) >= DOMINANT_SCROLL_SURFACE_WIDTH_RATIO &&
    visibleHeight / positiveNumber(win.innerHeight) >= DOMINANT_SCROLL_SURFACE_HEIGHT_RATIO
  );
}

function hasScrollableOverflow(style: CSSStyleDeclaration): boolean {
  return (
    style.overflowY === "auto" || style.overflowY === "scroll" || style.overflowY === "overlay"
  );
}

function isPaintedAndUntransformed(
  element: HTMLElement,
  doc: Document,
  win: ScreenshotPageWindow,
): boolean {
  let current: HTMLElement | null = element;
  while (current) {
    const style = win.getComputedStyle(current);
    if (
      style.display === "none" ||
      style.visibility === "hidden" ||
      style.contentVisibility === "hidden" ||
      (style.transform && style.transform !== "none")
    ) {
      return false;
    }
    if (current === doc.documentElement) break;
    current = current.parentElement;
  }
  return true;
}

function hasInlineStyle(element: Element): element is Element & ElementCSSInlineStyle {
  if (!("style" in element)) return false;
  const candidate = element as Element & Partial<ElementCSSInlineStyle>;
  return typeof candidate.style?.setProperty === "function";
}

function hideElement(element: Element & ElementCSSInlineStyle): HiddenElement {
  const hidden = {
    element,
    visibility: element.style.getPropertyValue("visibility"),
    priority: element.style.getPropertyPriority("visibility"),
  };
  element.style.setProperty("visibility", "hidden", "important");
  return hidden;
}

function restoreHiddenElements(elements: HiddenElement[]): void {
  for (const { element, visibility, priority } of elements) {
    if (visibility) {
      element.style.setProperty("visibility", visibility, priority);
    } else {
      element.style.removeProperty("visibility");
    }
  }
  elements.length = 0;
}

function isInViewport(element: Element, win: ScreenshotPageWindow): boolean {
  const bounds = element.getBoundingClientRect();
  return (
    bounds.bottom > 0 &&
    bounds.right > 0 &&
    bounds.top < win.innerHeight &&
    bounds.left < win.innerWidth
  );
}

function isFullyVisibleVertically(element: Element, win: ScreenshotPageWindow): boolean {
  const bounds = element.getBoundingClientRect();
  return bounds.top >= 0 && bounds.bottom <= win.innerHeight;
}

function finiteNumber(value: number): number {
  return Number.isFinite(value) ? value : 0;
}

function positiveNumber(value: number): number {
  return Number.isFinite(value) ? Math.max(1, value) : 1;
}
