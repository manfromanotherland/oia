// SPDX-License-Identifier: MIT

/** Raw structured page evidence; category and price interpretation live in Rust. */
export const MAX_PRODUCT_HTML_BYTES = 512 * 1024;
const MAX_STRUCTURED_DOM_DEPTH = 128;
const MAX_STRUCTURED_DOM_NODES = 16_384;
const STRUCTURED_DOM_LIMIT = Symbol("structured DOM limit");

const STRUCTURED_ATTRIBUTES = [
  "id",
  "class",
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
  "data-component",
  "data-purchase-title",
  "data-purchase-isbn13",
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

  const metadata: string[] = [];
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
      name.startsWith("book:") ||
      name.startsWith("music:") ||
      name.startsWith("video:") ||
      name.startsWith("og:image") ||
      name.startsWith("og:price:") ||
      [
        "og:type",
        "og:url",
        "og:title",
        "og:site_name",
        "og:description",
        "name",
        "title",
        "price",
        "pricecurrency",
      ].includes(name)
    ) {
      metadata.push(meta.outerHTML);
    }
  }
  const canonical = doc.querySelector("link[rel~='canonical']");
  if (canonical) metadata.push(canonical.outerHTML);
  if (!append(metadata.join("\n"))) return undefined;

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
  const candidates = Array.from(doc.querySelectorAll("[itemtype], [typeof]"));
  if (candidates.length > MAX_STRUCTURED_DOM_NODES) return undefined;
  for (const candidate of candidates) {
    let root = candidate;
    let depth = 0;
    for (let parent = candidate.parentElement; parent; parent = parent.parentElement) {
      if (++depth > MAX_STRUCTURED_DOM_DEPTH) return undefined;
      if (parent.matches("[itemscope], [typeof], [vocab], [prefix]")) root = parent;
    }
    roots.add(root);
  }
  // Keep every structured root and enclosing scope so a sibling article,
  // recommendation, review, or collection remains distinguishable from a
  // page's main product. Omit unrelated DOM/assets.
  let scopes: string;
  try {
    const budget = { nodes: 0 };
    scopes = Array.from(roots)
      .map((root) => cloneStructuredElement(root, budget)?.outerHTML)
      .filter((html): html is string => Boolean(html))
      .join("\n");
  } catch (error) {
    if (error === STRUCTURED_DOM_LIMIT) return undefined;
    throw error;
  }
  if (!append(scopes)) return undefined;

  // Preserve only public main-product fields; omit scripts, forms and private meta.
  try {
    const budget = { nodes: 0 };
    for (const field of Array.from(
      doc.querySelectorAll(
        "#productTitle, #ebooksProductTitle, #bylineInfo, #landingImage, #imgBlkFront, #priceBlock-outsideOfForm_feature_div, #corePrice_feature_div, #corePriceDisplay_desktop_feature_div",
      ),
    )) {
      if (!append(cloneStructuredElement(field, budget, true)?.outerHTML ?? "")) return undefined;
    }
  } catch (error) {
    if (error === STRUCTURED_DOM_LIMIT) return undefined;
    throw error;
  }

  // Raw provider evidence only: the core validates its host, route and identity.
  // Keep text and author links inside the main product header, never the page body.
  try {
    const budget = { nodes: 0 };
    for (const header of Array.from(
      doc.querySelectorAll('[data-component="heros:hero-product"]'),
    )) {
      if (!append(cloneStructuredElement(header, budget, true)?.outerHTML ?? "")) {
        return undefined;
      }
    }
  } catch (error) {
    if (error === STRUCTURED_DOM_LIMIT) return undefined;
    throw error;
  }

  return blocks.length ? blocks.join("\n") : undefined;
}

function cloneStructuredElement(
  element: Element,
  budget: { nodes: number },
  parentText = false,
  depth = 0,
): Element | undefined {
  if (++budget.nodes > MAX_STRUCTURED_DOM_NODES || depth > MAX_STRUCTURED_DOM_DEPTH) {
    throw STRUCTURED_DOM_LIMIT;
  }
  if (
    ["SCRIPT", "STYLE", "NOSCRIPT", "SVG", "IFRAME", "CANVAS", "INPUT"].includes(element.tagName) ||
    (parentText &&
      element.tagName === "META" &&
      !element.hasAttribute("itemprop") &&
      !element.hasAttribute("property"))
  ) {
    return undefined;
  }
  const scoped = element.hasAttribute("itemscope") || element.hasAttribute("typeof");
  const annotated = element.matches(
    "[itemscope], [itemtype], [itemprop], [itemid], [itemref], [typeof], [property], [resource], [about], [vocab], [prefix], [data-component], [data-purchase-isbn13]",
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
      const nested = cloneStructuredElement(child as Element, budget, captureText, depth + 1);
      if (nested) clone.appendChild(nested);
    }
  }
  return annotated || element.hasAttribute("id") || clone.childNodes.length ? clone : undefined;
}
