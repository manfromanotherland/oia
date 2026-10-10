// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";

import { CONTEXT_MENU, CONTEXT_MENU_ID, contextCaptureTarget } from "./context-menus.js";
import { HOST_ID } from "./host.js";
import { PROTOCOL_VERSION } from "./protocol.js";

describe("capture contract", () => {
  it("uses protocol version 4", () => {
    expect(PROTOCOL_VERSION).toBe(4);
  });

  it("targets the Óia native messaging host", () => {
    expect(HOST_ID).toBe("is.edmundo.cuttings.host");
  });

  it("exposes one generic context menu with the exact product label", () => {
    expect(CONTEXT_MENU).toEqual({
      id: CONTEXT_MENU_ID,
      title: "Save to Óia",
      contexts: ["page", "image", "video", "selection"],
    });
  });

  it("dispatches media, selected text, and plain pages from the click payload", () => {
    expect(contextCaptureTarget({ mediaType: "image", srcUrl: "https://e.com/image.jpg" })).toEqual(
      { kind: "image", mediaUrl: "https://e.com/image.jpg" },
    );
    expect(contextCaptureTarget({ mediaType: "video", srcUrl: "https://e.com/video.mp4" })).toEqual(
      { kind: "video", mediaUrl: "https://e.com/video.mp4" },
    );
    expect(contextCaptureTarget({ selectionText: "  A selected thought  " })).toEqual({
      kind: "selection",
      text: "A selected thought",
    });
    expect(contextCaptureTarget({})).toEqual({ kind: "article" });
  });

  it("prefers a clicked media item over selected text", () => {
    expect(
      contextCaptureTarget({
        mediaType: "image",
        srcUrl: "https://e.com/image.jpg",
        selectionText: "Nearby text",
      }),
    ).toEqual({ kind: "image", mediaUrl: "https://e.com/image.jpg" });
  });
});
