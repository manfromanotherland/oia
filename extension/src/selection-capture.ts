// SPDX-License-Identifier: MIT

import type { PageCapture } from "./content.js";
import { extractPage, type ExtractionResult } from "./extraction.js";
import { fetchImages } from "./images.js";
import { extractPageMetadata } from "./page-metadata.js";
import type { SelectionContext } from "./protocol.js";

export interface SelectionCapture extends PageCapture {
  selected_text: string;
  selection_context?: SelectionContext;
}

/** Cache strings/DOM facts while the menu opens; live Ranges change with the DOM. */
export class SelectionContextCache {
  private snapshot?: { url: string; context: SelectionContext };
  private menuCaptured = false;

  constructor(private readonly doc: Document) {}

  capture(target: EventTarget | null = null): void {
    this.menuCaptured = true;
    const element = target instanceof Element ? target : null;
    if (element?.closest("input, textarea, [contenteditable]:not([contenteditable='false'])")) {
      this.snapshot = undefined;
      return;
    }
    const context = readSelectionContext(this.doc);
    this.snapshot = context ? { url: this.doc.URL, context } : undefined;
  }

  forText(selectedText: string): SelectionContext | undefined {
    // A menu-time snapshot survives focus changes and DOM mutations, but must
    // never be reused for another selected passage or a navigated document.
    if (this.menuCaptured) {
      return this.snapshot?.url === this.doc.URL &&
        sameSelectedText(this.snapshot.context.selected_text, selectedText)
        ? this.snapshot.context
        : undefined;
    }
    const live = readSelectionContext(this.doc);
    return live && sameSelectedText(live.selected_text, selectedText) ? live : undefined;
  }
}

function sameSelectedText(left: string, right: string): boolean {
  const normalized = (text: string): string => text.replace(/\s+/gu, " ").trim();
  return normalized(left) === normalized(right);
}

/** Read DOM containment only. Quotation-mark rules and card classification live in Rust. */
export function readSelectionContext(doc: Document): SelectionContext | undefined {
  try {
    return selectionContextFromDom(doc);
  } catch {
    return undefined;
  }
}

function selectionContextFromDom(doc: Document): SelectionContext | undefined {
  if (
    doc.activeElement?.matches("input, textarea, [contenteditable]:not([contenteditable='false'])")
  ) {
    return undefined;
  }
  const selection = doc.getSelection();
  if (!selection || selection.rangeCount !== 1 || selection.isCollapsed) return undefined;
  const selectedText = selection.toString();
  if (!selectedText.trim()) return undefined;
  const range = selection.getRangeAt(0);
  if (range.startContainer.getRootNode() !== doc || range.endContainer.getRootNode() !== doc) {
    return undefined;
  }
  const root = range.commonAncestorContainer;
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let node: Node | null = root.nodeType === Node.TEXT_NODE ? root : walker.nextNode();
  let commonQuotes: Element[] | undefined;
  while (node) {
    if (range.intersectsNode(node)) {
      const text = (node as Text).data;
      const start = node === range.startContainer ? range.startOffset : 0;
      const end = node === range.endContainer ? range.endOffset : text.length;
      if (text.slice(start, end).trim()) {
        const ancestors: Element[] = [];
        for (let element = node.parentElement; element; element = element.parentElement) {
          if (element.matches("blockquote, q")) ancestors.push(element);
        }
        commonQuotes =
          commonQuotes === undefined
            ? ancestors
            : commonQuotes.filter((element) => ancestors.includes(element));
      }
    }
    node = walker.nextNode();
  }
  if (commonQuotes === undefined) return undefined;
  const enclosing = commonQuotes[0]?.localName;
  return {
    version: 1,
    selected_text: selectedText,
    ...(enclosing === "blockquote" || enclosing === "q" ? { enclosing_element: enclosing } : {}),
  };
}

/** Keep the selected text alongside an ordinary full article capture when possible. */
export async function captureSelection(
  doc: Document,
  pageUrl: string,
  selectedText: string,
  selectionContext?: SelectionContext,
  extract: (doc: Document, url: string) => ExtractionResult | null = extractPage,
): Promise<SelectionCapture> {
  let result: ExtractionResult | null = null;
  try {
    result = extract(doc, pageUrl);
  } catch {
    // The user's text remains saveable when Readability cannot extract a page.
  }
  if (!result?.markdown.trim()) {
    const page = extractPageMetadata(doc, pageUrl);
    result = {
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
      image_urls: [],
      preview_candidates: page.socialImageUrls,
      favicon_candidates: page.faviconUrls,
    };
  }
  const imageUrls = [
    ...new Set([...result.preview_candidates, ...result.favicon_candidates, ...result.image_urls]),
  ];
  const { images, unresolved, skipped } = await fetchImages(imageUrls);
  return {
    metadata: result.metadata,
    markdown: result.markdown,
    images,
    unresolved,
    skipped,
    preview_candidates: result.preview_candidates,
    favicon_candidates: result.favicon_candidates,
    content_image_urls: result.image_urls,
    selected_text: selectedText,
    ...(selectionContext && sameSelectedText(selectionContext.selected_text, selectedText)
      ? { selection_context: selectionContext }
      : {}),
  };
}
