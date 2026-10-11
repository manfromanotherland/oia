// SPDX-License-Identifier: MIT

import { describe, expect, it, vi } from "vitest";

import {
  createScreenshotSessionId,
  handleScreenshotPageMessage,
  isScreenshotPageMessage,
  SCREENSHOT_PAGE_BEGIN_ACTION,
  SCREENSHOT_PAGE_FINISH_ACTION,
  SCREENSHOT_PAGE_MEASURE_ACTION,
  SCREENSHOT_PAGE_SCROLL_ACTION,
  ScreenshotPageCaptureController,
} from "./screenshot-page.js";

interface PageFixture {
  controller: ScreenshotPageCaptureController;
  doc: Document;
  getScroll: () => { x: number; y: number };
  scrollTo: ReturnType<typeof vi.fn>;
  settle: ReturnType<typeof vi.fn>;
}

function pageFixture(
  sessionIds: string[] = ["capture-one"],
  settleImplementation: (() => Promise<void>) | null = async () => undefined,
  requestAnimationFrame: (callback: FrameRequestCallback) => number = (callback) => {
    callback(0);
    return 1;
  },
  pageHeight = 1_500,
): PageFixture {
  const doc = document.implementation.createHTMLDocument("Capture page");
  let scrollX = 12;
  let scrollY = 240;
  const viewportWidth = 800;
  const viewportHeight = 600;
  for (const element of [doc.documentElement, doc.body]) {
    Object.defineProperties(element, {
      scrollHeight: { configurable: true, value: pageHeight },
      offsetHeight: { configurable: true, value: pageHeight },
      clientHeight: { configurable: true, value: viewportHeight },
    });
  }

  const scrollTo = vi.fn((options: ScrollToOptions) => {
    scrollX = Number(options.left ?? scrollX);
    scrollY = Math.min(pageHeight - viewportHeight, Number(options.top ?? scrollY));
  });
  const settle = vi.fn(settleImplementation ?? (async () => undefined));
  const ids = [...sessionIds];
  const win = {
    innerWidth: viewportWidth,
    innerHeight: viewportHeight,
    get scrollX() {
      return scrollX;
    },
    get scrollY() {
      return scrollY;
    },
    location: { href: "https://example.com/long-page" },
    scrollTo,
    getComputedStyle: (element: Element) =>
      ({
        position: element.getAttribute("data-position") ?? "static",
        overflowY: element.getAttribute("data-overflow-y") ?? "visible",
      }) as CSSStyleDeclaration,
    requestAnimationFrame,
  };

  const options = {
    createSessionId: () => ids.shift() ?? "capture-fallback",
    ...(settleImplementation ? { settle } : {}),
  };

  return {
    controller: new ScreenshotPageCaptureController(doc, win, options),
    doc,
    getScroll: () => ({ x: scrollX, y: scrollY }),
    scrollTo,
    settle,
  };
}

function setBounds(
  element: Element,
  bounds: () => Pick<DOMRect, "top" | "right" | "bottom" | "left">,
): void {
  element.getBoundingClientRect = () => {
    const value = bounds();
    return {
      ...value,
      width: value.right - value.left,
      height: value.bottom - value.top,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect;
  };
}

describe("screenshot page capture", () => {
  it("creates a session id when randomUUID is unavailable on an HTTP page", () => {
    const getRandomValues = vi.fn((bytes: Uint8Array) => {
      bytes.set(Array.from({ length: 16 }, (_, index) => index));
      return bytes;
    });

    expect(createScreenshotSessionId({ getRandomValues })).toBe(
      "00010203-0405-4607-8809-0a0b0c0d0e0f",
    );
    expect(getRandomValues).toHaveBeenCalledOnce();
  });

  it("freezes the page, hides repeated overlays after the first tile, and restores it", async () => {
    const fixture = pageFixture();
    const fixed = fixture.doc.createElement("header");
    fixed.dataset.position = "fixed";
    fixed.style.setProperty("visibility", "visible", "important");
    const sticky = fixture.doc.createElement("nav");
    sticky.dataset.position = "sticky";
    const toast = fixture.doc.createElement("div");
    toast.id = "oia-toast-host";
    const ordinary = fixture.doc.createElement("main");
    const laterSticky = fixture.doc.createElement("aside");
    laterSticky.dataset.position = "sticky";
    const partialSticky = fixture.doc.createElement("section");
    partialSticky.dataset.position = "sticky";
    for (const element of [fixed, sticky, toast]) {
      setBounds(element, () => ({ top: 0, right: 800, bottom: 40, left: 0 }));
    }
    setBounds(laterSticky, () =>
      fixture.getScroll().y < 500
        ? { top: 1_000, right: 800, bottom: 1_040, left: 0 }
        : { top: 100, right: 800, bottom: 140, left: 0 },
    );
    setBounds(partialSticky, () =>
      fixture.getScroll().y < 500
        ? { top: 580, right: 800, bottom: 640, left: 0 }
        : { top: 0, right: 800, bottom: 60, left: 0 },
    );
    fixture.doc.body.append(fixed, sticky, toast, ordinary, laterSticky, partialSticky);

    const begun = await fixture.controller.begin();

    expect(begun).toEqual({
      sessionId: "capture-one",
      url: "https://example.com/long-page",
      viewportWidth: 800,
      viewportHeight: 600,
      documentHeight: 1_500,
      scrollY: 240,
    });
    const captureStyle = fixture.doc.querySelector<HTMLStyleElement>(
      "style[data-oia-screenshot-capture]",
    );
    expect(captureStyle?.textContent).toContain("scroll-behavior: auto !important");
    expect(captureStyle?.textContent).toContain("scroll-snap-type: none !important");
    expect(captureStyle?.textContent).toContain("animation-play-state: paused !important");
    expect(fixed.style.getPropertyValue("visibility")).toBe("visible");

    await fixture.controller.scrollTo("capture-one", 0, false);
    expect(fixed.style.getPropertyValue("visibility")).toBe("visible");
    expect(toast.style.getPropertyValue("visibility")).toBe("hidden");
    expect(partialSticky.style.getPropertyValue("visibility")).toBe("hidden");

    const scrolled = await fixture.controller.scrollTo("capture-one", 600, true);
    expect(scrolled.scrollY).toBe(600);
    expect(fixed.style.getPropertyValue("visibility")).toBe("hidden");
    expect(sticky.style.getPropertyValue("visibility")).toBe("hidden");
    expect(toast.style.getPropertyValue("visibility")).toBe("hidden");
    expect(ordinary.style.getPropertyValue("visibility")).toBe("");
    expect(laterSticky.style.getPropertyValue("visibility")).toBe("");
    expect(partialSticky.style.getPropertyValue("visibility")).toBe("");

    await fixture.controller.scrollTo("capture-one", 900, true);
    expect(laterSticky.style.getPropertyValue("visibility")).toBe("hidden");
    expect(partialSticky.style.getPropertyValue("visibility")).toBe("hidden");

    await fixture.controller.finish("capture-one");

    expect(fixture.doc.querySelector("style[data-oia-screenshot-capture]")).toBeNull();
    expect(fixed.style.getPropertyValue("visibility")).toBe("visible");
    expect(fixed.style.getPropertyPriority("visibility")).toBe("important");
    expect(sticky.style.getPropertyValue("visibility")).toBe("");
    expect(toast.style.getPropertyValue("visibility")).toBe("");
    expect(laterSticky.style.getPropertyValue("visibility")).toBe("");
    expect(partialSticky.style.getPropertyValue("visibility")).toBe("");
    expect(fixture.getScroll()).toEqual({ x: 12, y: 240 });
    expect(fixture.scrollTo).toHaveBeenLastCalledWith({ left: 12, top: 240, behavior: "auto" });
    expect(fixture.settle).toHaveBeenCalledTimes(8);
  });

  it("restores an interrupted session before beginning another and rejects stale ids", async () => {
    const fixture = pageFixture(["capture-one", "capture-two"]);
    const fixed = fixture.doc.createElement("header");
    fixed.dataset.position = "fixed";
    setBounds(fixed, () => ({ top: 0, right: 800, bottom: 40, left: 0 }));
    fixture.doc.body.append(fixed);

    await fixture.controller.begin();
    await fixture.controller.scrollTo("capture-one", 0, false);
    await fixture.controller.scrollTo("capture-one", 700, true);
    expect(fixed.style.getPropertyValue("visibility")).toBe("hidden");

    const replacement = await fixture.controller.begin();
    expect(replacement.sessionId).toBe("capture-two");
    expect(replacement.scrollY).toBe(240);
    expect(fixed.style.getPropertyValue("visibility")).toBe("");
    expect(fixture.doc.querySelectorAll("style[data-oia-screenshot-capture]")).toHaveLength(1);

    await expect(fixture.controller.scrollTo("capture-one", 0, false)).rejects.toThrow(
      "session is no longer active",
    );
    await expect(fixture.controller.finish("capture-one")).rejects.toThrow(
      "session is no longer active",
    );

    await fixture.controller.finish("capture-two");
    expect(fixture.doc.querySelector("style[data-oia-screenshot-capture]")).toBeNull();
  });

  it.each(["hidden", "clip", "visible"])(
    "scrolls and restores a full-viewport app surface containing overflow:%s content",
    async (overflowY) => {
      const fixture = pageFixture(["capture-one"], async () => undefined, undefined, 600);
      const fixedShell = fixture.doc.createElement("div");
      fixedShell.dataset.position = "fixed";
      setBounds(fixedShell, () => ({ top: 0, right: 800, bottom: 600, left: 0 }));
      const stationaryHeader = fixture.doc.createElement("header");
      setBounds(stationaryHeader, () => ({ top: 0, right: 800, bottom: 64, left: 0 }));
      const scroller = fixture.doc.createElement("main");
      scroller.dataset.overflowY = "auto";
      Object.defineProperties(scroller, {
        scrollHeight: { configurable: true, value: 1_500 },
        clientHeight: { configurable: true, value: 600 },
        clientWidth: { configurable: true, value: 800 },
        scrollTop: { configurable: true, writable: true, value: 240 },
        scrollLeft: { configurable: true, writable: true, value: 12 },
      });
      setBounds(scroller, () => ({ top: 0, right: 800, bottom: 600, left: 0 }));
      const imageContainer = fixture.doc.createElement("div");
      imageContainer.dataset.overflowY = overflowY;
      Object.defineProperties(imageContainer, {
        scrollHeight: { configurable: true, value: 458 },
        clientHeight: { configurable: true, value: 436 },
      });
      setBounds(imageContainer, () => ({ top: 64, right: 776, bottom: 500, left: 24 }));
      scroller.append(imageContainer);
      fixedShell.append(stationaryHeader, scroller);
      fixture.doc.body.append(fixedShell);

      await expect(fixture.controller.begin()).resolves.toMatchObject({
        documentHeight: 1_500,
        scrollY: 240,
      });
      await fixture.controller.scrollTo("capture-one", 0, false);
      await expect(fixture.controller.scrollTo("capture-one", 600, true)).resolves.toMatchObject({
        documentHeight: 1_500,
        scrollY: 600,
      });
      expect(scroller.scrollTop).toBe(600);
      expect(scroller.style.visibility).toBe("");
      expect(fixedShell.style.visibility).toBe("");
      expect(stationaryHeader.style.visibility).toBe("hidden");
      expect(fixture.getScroll()).toEqual({ x: 12, y: 240 });

      await fixture.controller.finish("capture-one");
      expect(scroller.scrollTop).toBe(240);
      expect(scroller.scrollLeft).toBe(12);
      expect(stationaryHeader.style.visibility).toBe("");
      expect(fixture.scrollTo).not.toHaveBeenCalled();
    },
  );

  it.each(["hidden", "clip", "visible"])(
    "captures a scrolling document with a dominant overflow:%s container",
    async (overflowY) => {
      const fixture = pageFixture();
      const imageContainer = fixture.doc.createElement("div");
      imageContainer.dataset.overflowY = overflowY;
      Object.defineProperties(imageContainer, {
        scrollHeight: { configurable: true, value: 458 },
        clientHeight: { configurable: true, value: 436 },
      });
      setBounds(imageContainer, () => ({ top: 64, right: 776, bottom: 500, left: 24 }));
      fixture.doc.body.append(imageContainer);

      await expect(fixture.controller.begin()).resolves.toMatchObject({ documentHeight: 1_500 });
      await expect(fixture.controller.scrollTo("capture-one", 600, false)).resolves.toMatchObject({
        scrollY: 600,
      });
      expect(fixture.scrollTo).toHaveBeenLastCalledWith({ left: 12, top: 600, behavior: "auto" });
      expect(imageContainer.scrollTop).toBe(0);

      await fixture.controller.finish("capture-one");
      expect(fixture.getScroll()).toEqual({ x: 12, y: 240 });
    },
  );

  it("rejects a dominant overflow:hidden surface without another page scroller", async () => {
    const fixture = pageFixture(["capture-one"], async () => undefined, undefined, 600);
    const scroller = fixture.doc.createElement("main");
    scroller.dataset.overflowY = "hidden";
    Object.defineProperties(scroller, {
      scrollHeight: { configurable: true, value: 1_500 },
      clientHeight: { configurable: true, value: 600 },
    });
    setBounds(scroller, () => ({ top: 0, right: 800, bottom: 600, left: 0 }));
    fixture.doc.body.append(scroller);

    await expect(fixture.controller.begin()).rejects.toThrow("scrolling layout");
  });

  it.each([600, 1_500])("rejects a full-viewport iframe on a %ipx document", async (pageHeight) => {
    const fixture = pageFixture(["capture-one"], async () => undefined, undefined, pageHeight);
    const iframe = fixture.doc.createElement("iframe");
    setBounds(iframe, () => ({ top: 0, right: 800, bottom: 600, left: 0 }));
    fixture.doc.body.append(iframe);

    await expect(fixture.controller.begin()).rejects.toThrow(
      pageHeight > 600 ? "competing scrolling surfaces" : "scrolling layout",
    );
  });

  it("rejects a dominant scrolling panel that does not fill the viewport", async () => {
    const fixture = pageFixture(["capture-one"], async () => undefined, undefined, 600);
    const scroller = fixture.doc.createElement("main");
    scroller.dataset.overflowY = "auto";
    Object.defineProperties(scroller, {
      scrollHeight: { configurable: true, value: 1_500 },
      clientHeight: { configurable: true, value: 536 },
    });
    setBounds(scroller, () => ({ top: 64, right: 800, bottom: 600, left: 0 }));
    fixture.doc.body.append(scroller);

    await expect(fixture.controller.begin()).rejects.toThrow("scrolling layout");
    expect(fixture.doc.querySelector("style[data-oia-screenshot-capture]")).toBeNull();
  });

  it("rejects an app scroller that occupies exactly half the viewport width", async () => {
    const fixture = pageFixture(["capture-one"], async () => undefined, undefined, 600);
    const scroller = fixture.doc.createElement("main");
    scroller.dataset.overflowY = "auto";
    Object.defineProperties(scroller, {
      scrollHeight: { configurable: true, value: 1_500 },
      clientHeight: { configurable: true, value: 536 },
    });
    setBounds(scroller, () => ({ top: 0, right: 800, bottom: 600, left: 400 }));
    fixture.doc.body.append(scroller);

    await expect(fixture.controller.begin()).rejects.toThrow("scrolling layout");
  });

  it("rejects a full-size scroll surface shifted away from the viewport edges", async () => {
    const fixture = pageFixture(["capture-one"], async () => undefined, undefined, 600);
    const scroller = fixture.doc.createElement("main");
    scroller.dataset.overflowY = "auto";
    Object.defineProperties(scroller, {
      scrollHeight: { configurable: true, value: 1_500 },
      clientHeight: { configurable: true, value: 600 },
    });
    setBounds(scroller, () => ({ top: 4, right: 800, bottom: 604, left: 0 }));
    fixture.doc.body.append(scroller);

    await expect(fixture.controller.begin()).rejects.toThrow("scrolling layout");
  });

  it("rejects a dominant nested scroller when the document also scrolls", async () => {
    const fixture = pageFixture(["capture-one"], async () => undefined, undefined, 650);
    const scroller = fixture.doc.createElement("main");
    scroller.dataset.overflowY = "auto";
    Object.defineProperties(scroller, {
      scrollHeight: { configurable: true, value: 1_500 },
      clientHeight: { configurable: true, value: 536 },
    });
    setBounds(scroller, () => ({ top: 64, right: 800, bottom: 600, left: 0 }));
    fixture.doc.body.append(scroller);

    await expect(fixture.controller.begin()).rejects.toThrow("competing scrolling surfaces");
  });

  it("rejects a bordered scroll surface whose content viewport is shorter than the screenshot", async () => {
    const fixture = pageFixture(["capture-one"], async () => undefined, undefined, 600);
    const scroller = fixture.doc.createElement("main");
    scroller.dataset.overflowY = "auto";
    Object.defineProperties(scroller, {
      scrollHeight: { configurable: true, value: 1_500 },
      clientHeight: { configurable: true, value: 596 },
    });
    setBounds(scroller, () => ({ top: 0, right: 800, bottom: 600, left: 0 }));
    fixture.doc.body.append(scroller);

    await expect(fixture.controller.begin()).rejects.toThrow("scrolling layout");
  });

  it("aborts when a selected app scroll surface is replaced", async () => {
    const fixture = pageFixture(["capture-one"], async () => undefined, undefined, 600);
    const scroller = fixture.doc.createElement("main");
    scroller.dataset.overflowY = "auto";
    Object.defineProperties(scroller, {
      scrollHeight: { configurable: true, value: 1_500 },
      clientHeight: { configurable: true, value: 600 },
      scrollTop: { configurable: true, writable: true, value: 240 },
      scrollLeft: { configurable: true, writable: true, value: 0 },
    });
    setBounds(scroller, () => ({ top: 0, right: 800, bottom: 600, left: 0 }));
    fixture.doc.body.append(scroller);

    await fixture.controller.begin();
    scroller.remove();
    await expect(fixture.controller.scrollTo("capture-one", 600, false)).rejects.toThrow(
      "scrolling surface changed",
    );
    await fixture.controller.finish("capture-one");
  });

  it("validates capture messages and returns stale-session errors to the worker", async () => {
    const fixture = pageFixture();
    expect(isScreenshotPageMessage({ action: SCREENSHOT_PAGE_BEGIN_ACTION })).toBe(true);
    expect(
      isScreenshotPageMessage({
        action: SCREENSHOT_PAGE_SCROLL_ACTION,
        sessionId: "capture-one",
        scrollY: 600,
        hideFixed: true,
      }),
    ).toBe(true);
    expect(
      isScreenshotPageMessage({
        action: SCREENSHOT_PAGE_SCROLL_ACTION,
        sessionId: "capture-one",
        scrollY: Number.NaN,
        hideFixed: true,
      }),
    ).toBe(false);
    expect(
      isScreenshotPageMessage({
        action: SCREENSHOT_PAGE_MEASURE_ACTION,
        sessionId: "capture-one",
      }),
    ).toBe(true);

    expect(
      await handleScreenshotPageMessage(fixture.controller, {
        action: SCREENSHOT_PAGE_FINISH_ACTION,
        sessionId: "stale-session",
      }),
    ).toEqual({ error: "The screenshot capture session is no longer active." });
  });

  it("cleans up immediately when capture setup cannot settle", async () => {
    const fixture = pageFixture(["capture-one"], async () => {
      throw new Error("settle failed");
    });

    await expect(fixture.controller.begin()).rejects.toThrow("settle failed");

    expect(fixture.doc.querySelector("style[data-oia-screenshot-capture]")).toBeNull();
    expect(fixture.getScroll()).toEqual({ x: 12, y: 240 });
  });

  it("does not re-hide page elements after a concurrent restore", async () => {
    let releaseScroll!: () => void;
    let settleCall = 0;
    const fixture = pageFixture(["capture-one"], () => {
      settleCall += 1;
      if (settleCall === 3) {
        return new Promise<void>((resolve) => {
          releaseScroll = resolve;
        });
      }
      return Promise.resolve();
    });
    const fixed = fixture.doc.createElement("header");
    fixed.dataset.position = "fixed";
    setBounds(fixed, () => ({ top: 0, right: 800, bottom: 40, left: 0 }));
    fixture.doc.body.append(fixed);

    await fixture.controller.begin();
    await fixture.controller.scrollTo("capture-one", 0, false);
    const interruptedScroll = fixture.controller.scrollTo("capture-one", 600, true);
    await Promise.resolve();

    await fixture.controller.finish("capture-one");
    releaseScroll();

    await expect(interruptedScroll).rejects.toThrow("session is no longer active");
    expect(fixed.style.getPropertyValue("visibility")).toBe("");
    expect(fixture.getScroll()).toEqual({ x: 12, y: 240 });
  });

  it("settles through a bounded fallback when animation frames are paused", async () => {
    vi.useFakeTimers();
    try {
      const fixture = pageFixture(["capture-one"], null, () => 1);
      const beginning = fixture.controller.begin();
      await vi.advanceTimersByTimeAsync(500);

      await expect(beginning).resolves.toMatchObject({ sessionId: "capture-one" });

      const finishing = fixture.controller.finish("capture-one");
      await vi.advanceTimersByTimeAsync(500);
      await expect(finishing).resolves.toBeUndefined();
      expect(fixture.doc.querySelector("style[data-oia-screenshot-capture]")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not hang when a visible lazy image cannot finish decoding", async () => {
    vi.useFakeTimers();
    try {
      const fixture = pageFixture(["capture-one"], null);
      const image = fixture.doc.createElement("img");
      image.src = "data:image/png;base64,AQID";
      Object.defineProperty(image, "complete", { configurable: true, value: true });
      image.decode = vi.fn(() => new Promise<void>(() => undefined));
      setBounds(image, () => ({ top: 100, right: 700, bottom: 500, left: 100 }));
      fixture.doc.body.append(image);

      const beginning = fixture.controller.begin();
      await vi.advanceTimersByTimeAsync(400);
      await expect(beginning).resolves.toMatchObject({ sessionId: "capture-one" });
      expect(image.decode).toHaveBeenCalledOnce();

      image.remove();
      const finishing = fixture.controller.finish("capture-one");
      await vi.advanceTimersByTimeAsync(100);
      await expect(finishing).resolves.toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
  });
});
