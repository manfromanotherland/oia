// SPDX-License-Identifier: MIT

import type { SaveKind, SaveRequestMetadata } from "./protocol.js";
import { absoluteUrl, documentBaseUrl, extractPageMetadata, firstText } from "./page-metadata.js";

export type StandaloneMediaKind = Extract<SaveKind, "image" | "video">;

const MAX_QUOTE_EXCERPT_CHARACTERS = 600;
const VIDEO_REFERENCE_SCHEME = "oia-video";

/** A standalone media item before its image bytes have been captured. */
export interface MediaExtractionResult {
  metadata: SaveRequestMetadata;
  markdown: string;
  /** Image URLs to capture. This is the image itself, or a video's poster. */
  image_urls: string[];
}

/**
 * Build a standalone image/video save from the live page DOM.
 *
 * This pure metadata extractor never requests video bytes: only an optional
 * poster is returned in `image_urls`. It retains durable or opaque source
 * metadata for extraction and legacy callers, while the production content
 * script intercepts every video and streams its bytes before an ordinary save
 * can persist a poster-only card.
 */
export function extractStandaloneMedia(
  doc: Document,
  pageUrl: string,
  kind: StandaloneMediaKind,
  mediaUrl: string,
  savedAt = new Date().toISOString(),
): MediaExtractionResult {
  const baseUrl = documentBaseUrl(doc, pageUrl);
  const resolvedMediaUrl = absoluteUrl(mediaUrl, baseUrl) ?? mediaUrl;
  const element = findMediaElement(doc, baseUrl, kind, resolvedMediaUrl);
  const origin = extractPageMetadata(doc, pageUrl);
  const caption = element ? figureCaption(element) : undefined;
  const video = element instanceof HTMLVideoElement ? element : undefined;
  const videoDestination =
    kind === "video"
      ? resolveVideoDestination(doc, pageUrl, baseUrl, origin.canonicalUrl, resolvedMediaUrl, video)
      : undefined;
  const storedMediaUrl = videoDestination?.mediaUrl ?? resolvedMediaUrl;
  const descriptiveMediaUrl =
    kind === "video" && !videoDestination?.isDirect ? undefined : storedMediaUrl;

  const elementLabel = firstText(
    element?.getAttribute("aria-label"),
    element?.getAttribute("title"),
  );
  const imageAlt =
    kind === "image" && element instanceof HTMLImageElement
      ? firstText(element.getAttribute("alt"))
      : undefined;
  // `title` always describes the source page when the page provides one. The
  // media-specific description stays in the Markdown alt text. A direct URL
  // remains useful extraction metadata; protocol-v4 imports remove it before
  // the begin message reaches the native host.
  const title = firstText(
    origin.title,
    caption,
    imageAlt,
    elementLabel,
    descriptiveMediaUrl ? filenameFromUrl(descriptiveMediaUrl) : undefined,
    kind === "image" ? "Saved image" : "Saved video",
  )!;
  const alt = firstText(
    imageAlt,
    caption,
    elementLabel,
    origin.title,
    descriptiveMediaUrl ? filenameFromUrl(descriptiveMediaUrl) : undefined,
    title,
  )!;

  const metadata: SaveRequestMetadata = {
    kind,
    url: pageUrl,
    canonical_url: origin.canonicalUrl,
    media_url: storedMediaUrl,
    title,
    saved_at: savedAt,
    ...(origin.author ? { author: origin.author } : {}),
    ...(origin.site ? { site: origin.site } : {}),
    ...(origin.themeColor ? { theme_color: origin.themeColor } : {}),
    ...(origin.lang ? { lang: origin.lang } : {}),
    ...(origin.excerpt ? { excerpt: origin.excerpt } : {}),
  };

  if (kind === "image") {
    return {
      metadata,
      markdown: `![${escapeMarkdownLabel(alt)}](${storedMediaUrl})`,
      image_urls: [storedMediaUrl],
    };
  }

  const posterUrl = firstText(
    video ? absoluteUrl(video.getAttribute("poster"), baseUrl) : undefined,
    origin.socialImageUrls[0],
  );
  const poster = posterUrl ? `![${escapeMarkdownLabel(alt)}](${posterUrl})\n\n` : "";

  return {
    metadata,
    markdown: `${poster}[Watch video](${videoDestination!.watchUrl})`,
    // Never include a video URL here: fetchImages is strictly for images/posters.
    image_urls: posterUrl ? [posterUrl] : [],
  };
}

interface VideoDestination {
  /** Durable direct URL, or a stable page-based reference used only for identity. */
  mediaUrl: string;
  /** Durable direct URL when available; otherwise the actual source page. */
  watchUrl: string;
  isDirect: boolean;
}

function resolveVideoDestination(
  doc: Document,
  pageUrl: string,
  baseUrl: string,
  canonicalUrl: string,
  clickedUrl: string,
  video: HTMLVideoElement | undefined,
): VideoDestination {
  const durableUrl = firstDurableHttpUrl(
    [clickedUrl, ...(video ? videoSourceUrls(video) : [])],
    baseUrl,
  );
  if (durableUrl) return { mediaUrl: durableUrl, watchUrl: durableUrl, isDirect: true };

  return {
    mediaUrl: stableVideoReference(doc, canonicalUrl, pageUrl, video),
    watchUrl: pageUrl,
    isDirect: false,
  };
}

function firstDurableHttpUrl(
  candidates: Array<string | null | undefined>,
  pageUrl: string,
): string | undefined {
  for (const candidate of candidates) {
    if (!candidate) continue;
    try {
      const url = new URL(candidate, pageUrl);
      if (url.protocol === "http:" || url.protocol === "https:") return url.href;
    } catch {
      // Ignore malformed and session-local candidates; the page fallback is stable.
    }
  }
  return undefined;
}

function stableVideoReference(
  doc: Document,
  canonicalUrl: string,
  pageUrl: string,
  video: HTMLVideoElement | undefined,
): string {
  const videos = Array.from(doc.querySelectorAll("video"));
  const index = video ? videos.indexOf(video) : -1;
  const identity = index >= 0 ? String(index + 1) : "unmatched";
  const origin = canonicalUrl || pageUrl;

  // This must remain non-HTTP: the core intentionally strips fragments while
  // normalizing HTTP(S) URLs, which would collapse every fallback on a page to
  // the same identity. The opaque reference is identity-only and never linked.
  return `${VIDEO_REFERENCE_SCHEME}:${encodeURIComponent(origin)}:${identity}`;
}

/** True when a video URL is valid only inside the current browser session/document. */
export function isSessionLocalVideoUrl(value: string): boolean {
  try {
    const protocol = new URL(value).protocol;
    return protocol === "blob:" || protocol === "data:";
  } catch {
    return /^(?:blob|data):/i.test(value.trim());
  }
}

/** Build a selected-text save as one continuous, paragraph-safe Markdown quote. */
export function extractQuote(
  doc: Document,
  pageUrl: string,
  selectedText: string,
  savedAt = new Date().toISOString(),
): MediaExtractionResult {
  const origin = extractPageMetadata(doc, pageUrl);
  const text = selectedText.replace(/\r\n?/g, "\n").trim();
  const excerpt = truncateUnicode(text.replace(/\s+/g, " ").trim(), MAX_QUOTE_EXCERPT_CHARACTERS);
  const markdown = text
    .split("\n")
    .map((line) => (line ? `> ${line}` : ">"))
    .join("\n");

  const metadata: SaveRequestMetadata = {
    kind: "quote",
    url: pageUrl,
    canonical_url: origin.canonicalUrl,
    title: origin.title ?? "Saved quote",
    saved_at: savedAt,
    excerpt,
    word_count: countWords(text),
    ...(origin.author ? { author: origin.author } : {}),
    ...(origin.site ? { site: origin.site } : {}),
    ...(origin.themeColor ? { theme_color: origin.themeColor } : {}),
    ...(origin.lang ? { lang: origin.lang } : {}),
  };

  return { metadata, markdown, image_urls: [] };
}

function findMediaElement(
  doc: Document,
  pageUrl: string,
  kind: StandaloneMediaKind,
  mediaUrl: string,
): HTMLImageElement | HTMLVideoElement | undefined {
  const expected = absoluteUrl(mediaUrl, pageUrl) ?? mediaUrl;
  const selector = kind === "image" ? "img" : "video";
  const elements = Array.from(doc.querySelectorAll<HTMLImageElement | HTMLVideoElement>(selector));

  return elements.find((element) => {
    const candidates =
      kind === "video"
        ? videoSourceUrls(element as HTMLVideoElement)
        : [element.getAttribute("src"), "currentSrc" in element ? element.currentSrc : undefined];
    return candidates.some((candidate) => absoluteUrl(candidate, pageUrl) === expected);
  });
}

function videoSourceUrls(video: HTMLVideoElement): Array<string | null | undefined> {
  return [
    video.currentSrc,
    video.getAttribute("src"),
    ...Array.from(video.querySelectorAll("source"), (source) => source.getAttribute("src")),
  ];
}

function figureCaption(element: Element): string | undefined {
  return firstText(element.closest("figure")?.querySelector("figcaption")?.textContent);
}

function filenameFromUrl(url: string): string | undefined {
  try {
    const filename = new URL(url).pathname.split("/").filter(Boolean).at(-1);
    if (!filename) return undefined;
    const decoded = decodeURIComponent(filename).replace(/\.[a-z0-9]{1,8}$/i, "");
    return firstText(decoded.replace(/[-_]+/g, " "));
  } catch {
    return undefined;
  }
}

function escapeMarkdownLabel(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/\[/g, "\\[").replace(/\]/g, "\\]");
}

function countWords(text: string): number {
  return text.trim().split(/\s+/).filter(Boolean).length;
}

/** Bound a card preview by Unicode code points without splitting surrogate pairs. */
function truncateUnicode(value: string, maxCharacters: number): string {
  const characters = Array.from(value);
  if (characters.length <= maxCharacters) return value;
  return `${characters
    .slice(0, maxCharacters - 1)
    .join("")
    .trimEnd()}…`;
}
