// SPDX-License-Identifier: MIT

import { captureProductHtml } from "./product-html.js";

/** Metadata and durable image roles exposed by the current page's live DOM. */
export interface PageMetadata {
  title?: string;
  canonicalUrl: string;
  site?: string;
  author?: string;
  lang?: string;
  excerpt?: string;
  themeColor?: string;
  /** Bounded live structured evidence interpreted by the Rust product parser. */
  productHtml?: string;
  socialImageUrls: string[];
  faviconUrls: string[];
}

/**
 * Read common HTML, Open Graph, and Twitter card metadata without fetching.
 * Image bytes are captured separately by the existing browser-side image
 * pipeline so the stored reading remains fully local.
 */
export function extractPageMetadata(doc: Document, pageUrl: string): PageMetadata {
  const canonicalUrl = absoluteUrl(
    firstText(
      doc.querySelector<HTMLLinkElement>("link[rel~='canonical']")?.getAttribute("href"),
      metaContent(doc, "meta[property='og:url']"),
    ),
    pageUrl,
  );
  const resolvedCanonical = isHttpUrl(canonicalUrl) ? canonicalUrl! : pageUrl;

  return {
    title: firstText(
      metaContent(doc, "meta[property='og:title']"),
      metaContent(doc, "meta[name='twitter:title']"),
      doc.title,
    ),
    canonicalUrl: resolvedCanonical,
    site: firstText(metaContent(doc, "meta[property='og:site_name']"), hostname(resolvedCanonical)),
    author: firstText(
      metaContent(doc, "meta[name='author']"),
      metaContent(doc, "meta[property='article:author']"),
    ),
    lang: normalizedLanguage(
      firstText(doc.documentElement.lang, metaContent(doc, "meta[property='og:locale']")),
    ),
    excerpt: firstText(
      metaContent(doc, "meta[property='og:description']"),
      metaContent(doc, "meta[name='twitter:description']"),
      metaContent(doc, "meta[name='description']"),
    ),
    themeColor: themeColor(doc),
    productHtml: captureProductHtml(doc),
    socialImageUrls: httpAssetUrls(
      [
        metaContent(doc, "meta[property='og:image:secure_url']"),
        metaContent(doc, "meta[property='og:image']"),
        metaContent(doc, "meta[property='og:image:url']"),
        metaContent(doc, "meta[name='twitter:image']"),
        metaContent(doc, "meta[name='twitter:image:src']"),
      ],
      pageUrl,
    ),
    faviconUrls: faviconUrls(doc, pageUrl),
  };
}

function themeColor(doc: Document): string | undefined {
  const candidates = Array.from(
    doc.querySelectorAll<HTMLMetaElement>("meta[name='theme-color'], meta[name='theme_color']"),
    (meta) => meta.getAttribute("content"),
  );
  return firstText(...candidates);
}

export function firstText(...values: Array<string | null | undefined>): string | undefined {
  for (const value of values) {
    const compact = value?.replace(/\s+/g, " ").trim();
    if (compact) return compact;
  }
  return undefined;
}

export function absoluteUrl(value: string | null | undefined, base: string): string | undefined {
  if (!value) return undefined;
  try {
    return new URL(value, base).href;
  } catch {
    return undefined;
  }
}

function faviconUrls(doc: Document, pageUrl: string): string[] {
  const candidates = Array.from(doc.querySelectorAll<HTMLLinkElement>("link[href]"))
    .filter((link) => {
      const rel = link.rel.toLowerCase().split(/\s+/);
      const type = link.type.toLowerCase().split(";")[0].trim();
      const supportedType = !type || (type.startsWith("image/") && type !== "image/svg+xml");
      return supportedType && (rel.includes("icon") || rel.includes("apple-touch-icon"));
    })
    .map((link, index) => ({
      url: httpAssetUrl(link.getAttribute("href"), pageUrl),
      size: declaredIconSize(link.getAttribute("sizes") ?? ""),
      index,
    }))
    .filter((candidate): candidate is { url: string; size: number; index: number } =>
      Boolean(candidate.url),
    )
    .sort((left, right) => right.size - left.size || right.index - left.index);

  return [...new Set(candidates.map((candidate) => candidate.url))];
}

function declaredIconSize(value: string): number {
  if (/\bany\b/i.test(value)) return Number.MAX_SAFE_INTEGER;
  let largest = 0;
  for (const match of value.matchAll(/(\d+)x(\d+)/gi)) {
    largest = Math.max(largest, Number(match[1]) * Number(match[2]));
  }
  return largest;
}

function httpAssetUrl(value: string | null | undefined, base: string): string | undefined {
  const resolved = absoluteUrl(value, base);
  if (!isHttpUrl(resolved)) return undefined;
  const parsed = new URL(resolved);
  return parsed.username || parsed.password ? undefined : parsed.href;
}

function httpAssetUrls(values: Array<string | null | undefined>, base: string): string[] {
  return [
    ...new Set(
      values
        .map((value) => httpAssetUrl(value, base))
        .filter((value): value is string => Boolean(value)),
    ),
  ];
}

function isHttpUrl(value: string | undefined): value is string {
  if (!value) return false;
  try {
    const protocol = new URL(value).protocol;
    return protocol === "http:" || protocol === "https:";
  } catch {
    return false;
  }
}

function hostname(url: string): string | undefined {
  try {
    return new URL(url).hostname;
  } catch {
    return undefined;
  }
}

function metaContent(doc: Document, selector: string): string | undefined {
  return firstText(doc.querySelector<HTMLMetaElement>(selector)?.content);
}

function normalizedLanguage(value: string | undefined): string | undefined {
  return value?.replace(/_/g, "-");
}
