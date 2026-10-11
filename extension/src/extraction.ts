// SPDX-License-Identifier: MIT

import { Readability } from "@mozilla/readability";
import TurndownService from "turndown";

import { extractPageMetadata, firstText } from "./page-metadata.js";
import type { PageCaptureEvidence, SaveRequestMetadata } from "./protocol.js";
import { applySiteAdapters } from "./site-adapters/index.js";

export interface ExtractionResult {
  metadata: SaveRequestMetadata;
  markdown: string;
  image_urls: string[];
  preview_candidates: string[];
  favicon_candidates: string[];
  page_content?: PageCaptureEvidence;
}

/**
 * Run Readability against a cloned document and convert the result to Markdown.
 * Returns null if the page doesn't look like an article.
 */
export function extractPage(doc: Document, pageUrl: string): ExtractionResult | null {
  const clone = doc.cloneNode(true) as Document;
  preserveHeadings(clone);
  applySiteAdapters(clone, pageUrl);
  const article = new Readability(clone).parse();
  if (!article?.content) return null;

  const { markdown, imageUrls } = htmlToMarkdown(article.content);
  const page = extractPageMetadata(doc, pageUrl);

  const author = firstText(article.byline, page.author);
  const site = firstText(article.siteName, page.site);
  const lang = firstText(article.lang, page.lang);
  const excerpt = firstText(article.excerpt, page.excerpt);

  const metadata: SaveRequestMetadata = {
    kind: "article",
    url: pageUrl,
    canonical_url: page.canonicalUrl,
    title: firstText(article.title, page.title) ?? new URL(pageUrl).hostname,
    saved_at: new Date().toISOString(),
    ...(author ? { author } : {}),
    site,
    ...(page.themeColor ? { theme_color: page.themeColor } : {}),
    ...(page.productHtml ? { product_html: page.productHtml } : {}),
    ...(lang ? { lang } : {}),
    ...(excerpt ? { excerpt } : {}),
    word_count: countWords(article.textContent ?? ""),
  };

  return {
    metadata,
    markdown,
    image_urls: imageUrls,
    preview_candidates: page.socialImageUrls,
    favicon_candidates: page.faviconUrls,
    page_content: measurePageContent(doc, article.content),
  };
}

/** Raw measurements of extracted HTML; Rust decides how to classify the page. */
export function measurePageContent(doc: Document, html: string): PageCaptureEvidence {
  const template = doc.createElement("template");
  template.innerHTML = html;
  const content = template.content;
  const textLength = (node: Node): number =>
    Array.from((node.textContent ?? "").replace(/\s+/g, " ").trim()).length;
  return {
    text_characters: textLength(content),
    link_characters: Array.from(content.querySelectorAll("a")).reduce(
      (total, link) => total + textLength(link),
      0,
    ),
    paragraph_characters: Array.from(content.querySelectorAll("p")).slice(0, 1_024).map(textLength),
  };
}

/**
 * Strip class/id attributes from heading elements before Readability runs.
 *
 * Readability's `unlikelyCandidates` heuristic removes any node whose class/id
 * contains the token "header" (among others) before the article is scored.
 * Genuine in-article headings with class names like Substack's
 * `header-anchor-post` therefore get dropped, so the Markdown loses every `##`.
 * Headings are inherently content, so neutralizing their class/id keeps them
 * without affecting how the surrounding body is selected.
 */
function preserveHeadings(doc: Document): void {
  doc.querySelectorAll("h1, h2, h3, h4, h5, h6").forEach((heading) => {
    heading.removeAttribute("class");
    heading.removeAttribute("id");
  });
}

/** Convert an HTML string to CommonMark Markdown, collecting image URLs. */
export function htmlToMarkdown(html: string): { markdown: string; imageUrls: string[] } {
  const imageUrls: string[] = [];

  const td = new TurndownService({
    headingStyle: "atx",
    codeBlockStyle: "fenced",
    bulletListMarker: "-",
  });

  // In-page anchors (`href="#..."`) point at element ids within the original
  // page. The extracted Markdown keeps no such ids (CommonMark headings carry
  // none, and `preserveHeadings` strips them), so these links are dangling in
  // every viewer. Unwrap them to their text content — keeping footnote markers,
  // TOC labels, and "back to top" wording as plain prose without a dead link.
  td.addRule("inPageAnchors", {
    filter(node) {
      return node.nodeName === "A" && (node.getAttribute("href") ?? "").startsWith("#");
    },
    replacement(content) {
      return content;
    },
  });

  // The reading's own title is captured separately (in metadata) and rendered as
  // the article's sole h1. A body <h1> would be a second top-level heading, so
  // demote it to <h2> ("##") — one h1 per reading. Deeper levels (h2–h6) are left
  // as they are.
  td.addRule("demoteH1", {
    filter: "h1",
    replacement(content) {
      return `\n\n## ${content}\n\n`;
    },
  });

  td.addRule("images", {
    filter: "img",
    replacement(_content, node) {
      const img = node as HTMLImageElement;
      const src = img.getAttribute("src") ?? "";
      const alt = img.getAttribute("alt") ?? "";
      if (src && !src.startsWith("data:")) {
        imageUrls.push(src);
      }
      return src ? `![${alt}](${src})` : "";
    },
  });

  const markdown = td.turndown(html);
  return { markdown, imageUrls };
}

/** Count whitespace-delimited words in a plain-text string. */
export function countWords(text: string): number {
  return text.trim().split(/\s+/).filter(Boolean).length;
}
