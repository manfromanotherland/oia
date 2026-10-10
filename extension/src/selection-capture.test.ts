// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  captureSelection,
  readSelectionContext,
  SelectionContextCache,
} from "./selection-capture.js";
import { buildSaveSelectionRequest } from "./selection-request.js";

const { fetchImages } = vi.hoisted(() => ({
  fetchImages: vi.fn(async (_urls: string[]) => ({ images: [], unresolved: [] })),
}));
vi.mock("./images.js", () => ({ fetchImages }));

function select(
  start: Node,
  startOffset: number,
  end = start,
  endOffset = start.textContent!.length,
): void {
  const range = document.createRange();
  range.setStart(start, startOffset);
  range.setEnd(end, endOffset);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
}

afterEach(() => {
  document.getSelection()?.removeAllRanges();
  document.body.innerHTML = "";
  document.head.innerHTML = "";
  vi.clearAllMocks();
});

describe("selection DOM evidence", () => {
  it("retains a shared quote element across multiple paragraphs and nested inline elements", () => {
    document.body.innerHTML =
      "<blockquote><p>First <em>passage</em>.</p><p>Second passage.</p></blockquote>";
    const first = document.querySelector("p")!.firstChild!;
    const last = document.querySelectorAll("p")[1].firstChild!;
    select(first, 2, last, 6);
    expect(readSelectionContext(document)).toMatchObject({
      version: 1,
      enclosing_element: "blockquote",
    });
  });

  it("checks meaningful text rather than common ancestor or selection endpoints", () => {
    document.body.innerHTML = "<p><q>Quoted text</q><span> ordinary prose</span></p>";
    const paragraph = document.querySelector("p")!;
    select(paragraph, 0, paragraph, 1);
    expect(readSelectionContext(document)?.enclosing_element).toBe("q");
    const quote = document.querySelector("q")!.firstChild!;
    const prose = document.querySelector("span")!.firstChild!;
    select(quote, 3, prose, 9);
    expect(readSelectionContext(document)).toMatchObject({ version: 1 });
    expect(readSelectionContext(document)).not.toHaveProperty("enclosing_element");
  });

  it("ignores whitespace and zero-length text at exact block boundaries", () => {
    document.body.innerHTML = "<blockquote>Quoted passage</blockquote>   <p>Prose</p>";
    const quote = document.querySelector("blockquote")!.firstChild!;
    const prose = document.querySelector("p")!.firstChild!;
    select(quote, 0, prose, 0);
    expect(readSelectionContext(document)?.enclosing_element).toBe("blockquote");
  });

  it("does not treat two separate quotation elements as one enclosing quote", () => {
    document.body.innerHTML = "<q>First quote.</q><q>Second quote.</q>";
    const quotes = document.querySelectorAll("q");
    select(quotes[0].firstChild!, 0, quotes[1].firstChild!);
    expect(readSelectionContext(document)).not.toHaveProperty("enclosing_element");
  });

  it("uses the shared outer blockquote when a selection extends out of a nested q", () => {
    document.body.innerHTML = "<blockquote><q>Inner quote</q> outer text</blockquote>";
    select(
      document.querySelector("q")!.firstChild!,
      0,
      document.querySelector("blockquote")!.lastChild!,
    );
    expect(readSelectionContext(document)?.enclosing_element).toBe("blockquote");
  });

  it("keeps serialized menu-time facts after focus loss or DOM mutation", () => {
    document.body.innerHTML = "<blockquote>Selected passage</blockquote>";
    select(document.querySelector("blockquote")!.firstChild!, 0);
    const cache = new SelectionContextCache(document);
    cache.capture();
    document.getSelection()!.removeAllRanges();
    document.body.innerHTML = "<p>Selected passage</p>";
    expect(cache.forText("  Selected\npassage  ")?.enclosing_element).toBe("blockquote");
    expect(cache.forText("Different passage")).toBeUndefined();
  });

  it("clears old evidence on the next menu when there is no readable selection", () => {
    document.body.innerHTML = "<q>Selected passage</q>";
    const text = document.querySelector("q")!.firstChild!;
    select(text, 0);
    const cache = new SelectionContextCache(document);
    cache.capture();
    document.getSelection()!.removeAllRanges();
    cache.capture();
    select(text, 0);
    expect(cache.forText("Selected passage")).toBeUndefined();
  });

  it("only uses a live fallback when its text matches the clicked selection", () => {
    document.body.innerHTML = "<q>Selected passage</q>";
    select(document.querySelector("q")!.firstChild!, 0);
    const cache = new SelectionContextCache(document);
    expect(cache.forText("Selected passage")?.enclosing_element).toBe("q");
    expect(cache.forText("Unrelated passage")).toBeUndefined();
  });

  it("does not reuse an unrelated document selection for an editable context menu", () => {
    document.body.innerHTML = '<q>Selected passage</q><input value="Selected passage">';
    select(document.querySelector("q")!.firstChild!, 0);
    const cache = new SelectionContextCache(document);
    cache.capture(document.querySelector("input"));
    expect(cache.forText("Selected passage")).toBeUndefined();
  });
});

describe("selection article capture", () => {
  it("retains full article Markdown and captures its existing page image roles", async () => {
    const doc = document.implementation.createHTMLDocument("Source article");
    doc.head.insertAdjacentHTML(
      "beforeend",
      '<meta property="og:image" content="https://example.com/preview.png">',
    );
    doc.body.innerHTML = `<article><p>${"A sufficiently long source article paragraph about useful ideas. ".repeat(30)}</p><img src="https://example.com/body.png"></article>`;
    const selectedText = "A sufficiently long source article paragraph";
    const capture = await captureSelection(doc, "https://example.com/article", selectedText, {
      version: 1,
      selected_text: selectedText,
    });
    expect(capture.metadata.kind).toBe("article");
    expect(capture.markdown).toContain("useful ideas");
    expect(capture.markdown).not.toMatch(/^>/m);
    expect(capture.selected_text).toBe(selectedText);
    expect(fetchImages).toHaveBeenCalledWith([
      "https://example.com/preview.png",
      "https://example.com/body.png",
    ]);
    const request = buildSaveSelectionRequest(capture);
    expect(request.action).toBe("save_selection");
    expect(request.markdown).toBe(capture.markdown);
    expect(request.selected_text).toBe(selectedText);
    expect(request.selection_context?.version).toBe(1);
  });

  it.each(["empty", "throws"])(
    "preserves selected text and page metadata when extraction %s",
    async (failure) => {
      const doc = document.implementation.createHTMLDocument("Source page");
      doc.title = "Source page";
      doc.head.insertAdjacentHTML(
        "beforeend",
        '<link rel="canonical" href="https://example.com/canonical"><meta property="og:description" content="Source description"><meta name="theme-color" content="#123456"><meta property="og:image" content="https://example.com/social.png">',
      );
      const extract =
        failure === "empty"
          ? () => null
          : () => {
              throw new Error("Unreadable page");
            };
      const capture = await captureSelection(
        doc,
        "https://example.com/source",
        "Selected plain text",
        undefined,
        extract,
      );
      expect(capture.metadata).toMatchObject({
        kind: "article",
        title: "Source page",
        canonical_url: "https://example.com/canonical",
        excerpt: "Source description",
        theme_color: "#123456",
      });
      expect(capture.markdown).toBe("");
      expect(capture.selected_text).toBe("Selected plain text");
      expect(fetchImages).toHaveBeenCalledWith(["https://example.com/social.png"]);
      expect(buildSaveSelectionRequest(capture).action).toBe("save_selection");
    },
  );

  it("omits mismatched context rather than applying quote evidence to another passage", async () => {
    const doc = document.implementation.createHTMLDocument("Source");
    const capture = await captureSelection(
      doc,
      "https://example.com/article",
      "Current text",
      { version: 1, selected_text: "Previous text", enclosing_element: "q" },
      () => null,
    );
    expect(capture).not.toHaveProperty("selection_context");
  });

  it("does not classify quotation punctuation in JavaScript", async () => {
    const doc = document.implementation.createHTMLDocument("Source");
    const capture = await captureSelection(
      doc,
      "https://example.com/article",
      "“Quoted text”",
      undefined,
      () => null,
    );
    expect(capture.metadata.kind).toBe("article");
    expect(capture.selected_text).toBe("“Quoted text”");
    expect(capture.markdown).toBe("");
  });
});
