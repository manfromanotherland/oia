// SPDX-License-Identifier: MIT

import type { PageCapture } from "./content.js";
import type { SaveLinkRequest } from "./protocol.js";
import { PROTOCOL_VERSION } from "./protocol.js";

export const TOOLBAR_SAVE_ACTION = "toolbar-save";

export const TOOLBAR_SAVE_KINDS = ["page", "screenshot"] as const;

export type ToolbarSaveKind = (typeof TOOLBAR_SAVE_KINDS)[number];

export interface ToolbarSaveMessage {
  action: typeof TOOLBAR_SAVE_ACTION;
  kind: ToolbarSaveKind;
  tabId: number;
}

export function isToolbarSaveKind(value: unknown): value is ToolbarSaveKind {
  return typeof value === "string" && TOOLBAR_SAVE_KINDS.some((kind) => kind === value);
}

export function toolbarSaveMessage(kind: ToolbarSaveKind, tabId: number): ToolbarSaveMessage {
  return { action: TOOLBAR_SAVE_ACTION, kind, tabId };
}

export function isToolbarSaveMessage(value: unknown): value is ToolbarSaveMessage {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<ToolbarSaveMessage>;
  return (
    candidate.action === TOOLBAR_SAVE_ACTION &&
    isToolbarSaveKind(candidate.kind) &&
    Number.isInteger(candidate.tabId) &&
    candidate.tabId! > 0
  );
}

/** True when one visible-tile capture remained on the same settled browser document. */
export function isStableScreenshotDocument(
  before: Pick<chrome.tabs.Tab, "id" | "url" | "pendingUrl">,
  after: Pick<chrome.tabs.Tab, "id" | "url" | "pendingUrl">,
): boolean {
  return (
    before.id === after.id &&
    urlWithoutFragment(before.url) === urlWithoutFragment(after.url) &&
    !before.pendingUrl &&
    !after.pendingUrl
  );
}

function urlWithoutFragment(value: string | undefined): string | undefined {
  return value?.split("#", 1)[0];
}

/** Build a lightweight-link request from metadata/assets read from the live DOM. */
export function buildSaveLinkRequest(capture: PageCapture): SaveLinkRequest {
  if (capture.metadata.kind !== "article") {
    throw new Error("A lightweight link must use article metadata.");
  }
  assertHttpUrl(capture.metadata.url);
  return {
    protocol_version: PROTOCOL_VERSION,
    action: "save_link",
    metadata: capture.metadata,
    images: capture.images,
    preview_url: capture.preview_url,
    favicon_url: capture.favicon_url,
  };
}

/** Fall back to browser tab metadata when a page cannot run a content script. */
export function buildFallbackLinkCapture(
  tab: Pick<chrome.tabs.Tab, "title" | "url">,
  savedAt = new Date().toISOString(),
): PageCapture {
  const page = webPage(tab);
  return {
    metadata: {
      kind: "article",
      url: page.url,
      canonical_url: page.url,
      title: page.title,
      site: page.site,
      saved_at: savedAt,
    },
    markdown: "",
    images: [],
    unresolved: [],
    preview_candidates: [],
    favicon_candidates: [],
  };
}

/**
 * Turn the assembled full-page PNG data URL into a normal standalone-image
 * capture. The raw-byte hash is used as the media identity so data URLs never
 * leak into frontmatter and repeat captures deduplicate deterministically.
 */
export async function buildScreenshotCapture(
  tab: Pick<chrome.tabs.Tab, "title" | "url">,
  dataUrl: string,
  savedAt = new Date().toISOString(),
): Promise<PageCapture> {
  const page = webPage(tab);
  const dataBase64 = pngBase64(dataUrl);
  const bytes = decodeBase64(dataBase64);
  const hash = await sha256Hex(bytes);
  const mediaUrl = `cuttings-asset:assets/${hash}.png`;
  const alt = `Screenshot of ${page.title}`;

  return {
    metadata: {
      kind: "image",
      url: page.url,
      canonical_url: page.url,
      media_url: mediaUrl,
      title: page.title,
      site: page.site,
      saved_at: savedAt,
    },
    markdown: `![${escapeMarkdownLabel(alt)}](${mediaUrl})`,
    images: [
      {
        url: mediaUrl,
        content_type: "image/png",
        data_base64: dataBase64,
      },
    ],
    unresolved: [],
  };
}

interface WebPage {
  url: string;
  title: string;
  site: string;
}

function webPage(tab: Pick<chrome.tabs.Tab, "title" | "url">): WebPage {
  const rawUrl = tab.url?.trim();
  if (!rawUrl) throw new Error("The active tab has no URL.");

  const parsed = assertHttpUrl(rawUrl);

  const title = tab.title?.replace(/\s+/g, " ").trim() || parsed.hostname;
  return { url: parsed.href, title, site: parsed.hostname };
}

function assertHttpUrl(value: string): URL {
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error("The active tab does not have a valid web URL.");
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error("Only HTTP(S) pages can be saved to Óia.");
  }
  return parsed;
}

function pngBase64(dataUrl: string): string {
  const match = /^data:image\/png;base64,([a-z\d+/=]+)$/i.exec(dataUrl.trim());
  if (!match?.[1]) throw new Error("The browser did not return a PNG screenshot.");
  return match[1];
}

function decodeBase64(value: string): Uint8Array {
  let binary: string;
  try {
    binary = atob(value);
  } catch {
    throw new Error("The browser returned invalid screenshot data.");
  }
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const digestInput = new Uint8Array(bytes.byteLength);
  digestInput.set(bytes);
  const digest = await crypto.subtle.digest("SHA-256", digestInput.buffer);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function escapeMarkdownLabel(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/\[/g, "\\[").replace(/\]/g, "\\]");
}
