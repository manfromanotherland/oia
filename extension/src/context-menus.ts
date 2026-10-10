// SPDX-License-Identifier: MIT

export const CONTEXT_MENU_ID = "add-to-oia";

export const CONTEXT_MENU: chrome.contextMenus.CreateProperties = {
  id: CONTEXT_MENU_ID,
  title: "Save to Óia",
  contexts: ["page", "image", "video", "selection"],
};

export type ContextCaptureTarget =
  | { kind: "article" }
  | { kind: "image" | "video"; mediaUrl?: string }
  | { kind: "selection"; text: string };

/** Route one generic context-menu click to the selected thing. */
export function contextCaptureTarget(
  info: Pick<chrome.contextMenus.OnClickData, "mediaType" | "selectionText" | "srcUrl">,
): ContextCaptureTarget {
  if (info.mediaType === "image" || info.mediaType === "video") {
    return { kind: info.mediaType, mediaUrl: info.srcUrl };
  }

  const text = info.selectionText?.trim();
  if (text) return { kind: "selection", text };

  return { kind: "article" };
}
