// SPDX-License-Identifier: MIT

/** Raw structured page evidence; interpretation and price validation live in Rust. */
export const MAX_PRODUCT_HTML_BYTES = 512 * 1024;

const STRUCTURED_ATTRIBUTES = [
  "id",
  "itemscope",
  "itemtype",
  "itemprop",
  "itemid",
  "itemref",
  "typeof",
  "property",
  "resource",
  "about",
  "vocab",
  "prefix",
  "content",
  "value",
  "href",
  "src",
  "datetime",
];

/**
 * Preserve live JSON-LD graphs and scoped microdata/RDFa for the core parser.
 * Omit evidence when a graph or scope exceeds the wire budget, so a truncated
 * graph or collection cannot accidentally describe a single item.
 */
export function captureProductHtml(doc: Document): string | undefined {
  const blocks: string[] = [];
  const encoder = new TextEncoder();
  let bytes = 0;
  const append = (html: string): boolean => {
    if (!html) return true;
    const nextBytes = encoder.encode(html).byteLength + (blocks.length ? 1 : 0);
    if (bytes + nextBytes > MAX_PRODUCT_HTML_BYTES) return false;
    blocks.push(html);
    bytes += nextBytes;
    return true;
  };

  for (const meta of Array.from(
    doc.querySelectorAll("meta[property], meta[name], meta[itemprop]"),
  )) {
    const name = (
      meta.getAttribute("property") ??
      meta.getAttribute("name") ??
      meta.getAttribute("itemprop") ??
      ""
    ).toLowerCase();
    if (
      name.startsWith("product:") ||
      name.startsWith("og:price:") ||
      ["og:type", "og:url", "price", "pricecurrency"].includes(name)
    ) {
      append(meta.outerHTML);
    }
  }
  const canonical = doc.querySelector("link[rel~='canonical']");
  if (canonical) append(canonical.outerHTML);

  // Include every graph, including WebPage.mainEntity and ItemList context.
  const graphs = Array.from(doc.querySelectorAll("script[type]"))
    .filter(
      (script) =>
        script.getAttribute("type")?.split(";", 1)[0].trim().toLowerCase() ===
        "application/ld+json",
    )
    .map((script) => script.outerHTML)
    .join("\n");
  if (!append(graphs)) return undefined;

  const roots = new Set<Element>();
  for (const candidate of Array.from(doc.querySelectorAll("[itemtype], [typeof]"))) {
    let root = candidate;
    for (let parent = candidate.parentElement; parent; parent = parent.parentElement) {
      if (parent.matches("[itemscope], [typeof], [vocab], [prefix]")) root = parent;
    }
    roots.add(root);
  }
  // Keep every structured root and enclosing scope so a sibling article,
  // recommendation, review, or collection remains distinguishable from a
  // page's main product. Omit unrelated DOM/assets.
  const scopes = Array.from(roots)
    .map((root) => cloneStructuredElement(root)?.outerHTML)
    .filter((html): html is string => Boolean(html))
    .join("\n");
  if (!append(scopes)) return undefined;

  return blocks.length ? blocks.join("\n") : undefined;
}

function cloneStructuredElement(element: Element, parentText = false): Element | undefined {
  if (["SCRIPT", "STYLE", "NOSCRIPT", "SVG", "IFRAME", "CANVAS"].includes(element.tagName)) {
    return undefined;
  }
  const scoped = element.hasAttribute("itemscope") || element.hasAttribute("typeof");
  const annotated = element.matches(
    "[itemscope], [itemtype], [itemprop], [itemid], [itemref], [typeof], [property], [resource], [about], [vocab], [prefix]",
  );
  const captureText =
    !scoped && (parentText || element.hasAttribute("itemprop") || element.hasAttribute("property"));
  const clone = element.ownerDocument.createElement(element.localName);
  for (const name of STRUCTURED_ATTRIBUTES) {
    const value = element.getAttribute(name);
    if (value !== null && !(name === "src" && /^(?:data|blob):/i.test(value))) {
      clone.setAttribute(name, value);
    }
  }
  for (const child of Array.from(element.childNodes)) {
    if (child.nodeType === Node.TEXT_NODE && captureText) {
      clone.appendChild(child.cloneNode());
    } else if (child.nodeType === Node.ELEMENT_NODE) {
      const nested = cloneStructuredElement(child as Element, captureText);
      if (nested) clone.appendChild(nested);
    }
  }
  return annotated || clone.childNodes.length ? clone : undefined;
}
