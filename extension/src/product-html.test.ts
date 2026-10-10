// SPDX-License-Identifier: MIT

import { describe, expect, it } from "vitest";

import { captureProductHtml, MAX_PRODUCT_HTML_BYTES } from "./product-html.js";

function buildDoc(head: string, body = ""): Document {
  const doc = document.implementation.createHTMLDocument("Product");
  doc.head.insertAdjacentHTML("beforeend", head);
  doc.body.innerHTML = body;
  return doc;
}

describe("structured product evidence", () => {
  it("preserves cultural OG metadata, artwork alternatives and names as raw facts", () => {
    const doc = buildDoc(`<meta property="og:type" content="book">
      <meta property="og:title" content="The book">
      <meta property="og:image" content="https://example.com/cover.jpg">
      <meta property="og:image:secure_url" content="https://example.com/cover-secure.jpg">
      <meta property="book:isbn" content="9780143131144">
      <meta property="music:musician" content="https://example.com/artist">
      <meta property="video:director" content="https://example.com/director">
      <meta name="title" content="The browser title">
      <meta itemprop="name" content="The declared name">`);
    const evidence = captureProductHtml(doc)!;
    for (const meta of Array.from(
      doc.querySelectorAll("meta[property], meta[name], meta[itemprop]"),
    )) {
      expect(evidence).toContain(meta.outerHTML);
    }
  });

  it("keeps a non-OG cover on the structured main Book without extracting its category", () => {
    const doc = buildDoc(
      "",
      `<main itemscope itemtype="https://schema.org/WebPage">
      <div itemprop="mainEntity" itemscope itemtype="https://schema.org/Book">
        <span itemprop="name">A book</span><img itemprop="image" src="/cover.jpg">
      </div></main>`,
    );
    const evidence = captureProductHtml(doc)!;
    expect(evidence).toContain('itemprop="mainEntity"');
    expect(evidence).toContain('itemprop="image" src="/cover.jpg"');
  });

  it("omits an oversized metadata group atomically instead of dropping negative type context", () => {
    const doc = buildDoc(`<meta property="og:type" content="article">
      <meta property="og:description" content="${"x".repeat(MAX_PRODUCT_HTML_BYTES)}">
      <meta property="book:isbn" content="9780143131144">`);
    expect(captureProductHtml(doc)).toBeUndefined();
  });
  it("preserves separate live JSON-LD graphs and page identity without interpreting them", () => {
    const doc = buildDoc(`
      <link rel="canonical" href="/lamp">
      <meta property="og:url" content="https://example.com/lamp">
      <meta property="og:type" content="product">
      <meta property="product:price:amount" content="229.00">
      <meta property="product:price:currency" content="EUR">
      <script type="application/ld+json">{"@type":"WebPage","mainEntity":{"@id":"#lamp"}}</script>
      <script type="application/ld+json">{"@id":"#lamp","@type":"Product","offers":{"price":"229.00","priceCurrency":"EUR"}}</script>
      <script>window.privateState = "do not capture";</script>
    `);
    const liveScript = doc.querySelectorAll("script")[1];
    liveScript.textContent = liveScript.textContent!.replace("229.00", "219.00");

    const evidence = captureProductHtml(doc)!;
    for (const element of Array.from(doc.querySelectorAll("meta, link, script[type]"))) {
      expect(evidence).toContain(element.outerHTML);
    }
    expect(evidence).toContain('"price":"219.00"');
    expect(evidence).not.toContain("privateState");
    expect(doc.querySelector("script")!.textContent).toContain("mainEntity");
  });

  it("keeps microdata collection ancestry and nested offers while omitting unrelated assets", () => {
    const doc = buildDoc(
      "",
      `<main itemscope itemtype="https://schema.org/ItemList">
        <div itemprop="itemListElement" itemscope itemtype="https://schema.org/Product">
          <span itemprop="name">Lamp</span>
          <div itemprop="offers" itemscope itemtype="https://schema.org/Offer">
            <span itemprop="price"><b>229</b>.00</span>
            <meta itemprop="priceCurrency" content="EUR">
          </div>
          <img itemprop="image" src="data:image/png;base64,PRIVATE_BYTES">
          <script>PRIVATE_SCRIPT</script>
          <div>Unrelated page layout</div>
        </div>
      </main>`,
    );

    const evidence = captureProductHtml(doc)!;
    const captured = new DOMParser().parseFromString(evidence, "text/html");
    const product = captured.querySelector("[itemtype='https://schema.org/Product']")!;
    expect(product.parentElement!.getAttribute("itemtype")).toBe("https://schema.org/ItemList");
    expect(product.querySelector("[itemprop='price']")!.textContent).toBe("229.00");
    expect(product.querySelector("[itemprop='priceCurrency']")!.getAttribute("content")).toBe(
      "EUR",
    );
    expect(evidence).not.toContain("PRIVATE_BYTES");
    expect(evidence).not.toContain("PRIVATE_SCRIPT");
    expect(evidence).not.toContain("Unrelated page layout");
  });

  it("preserves inherited RDFa vocabulary and mainEntity scoping", () => {
    const doc = buildDoc(
      "",
      `<main vocab="https://schema.org/" typeof="WebPage">
        <div property="mainEntity" typeof="Product" resource="https://example.com/lamp">
          <span property="name">Lamp</span>
          <div property="offers" typeof="Offer">
            <meta property="price" content="229.00">
            <meta property="priceCurrency" content="EUR">
          </div>
        </div>
      </main>`,
    );

    const captured = new DOMParser().parseFromString(captureProductHtml(doc)!, "text/html");
    const product = captured.querySelector("[typeof='Product']")!;
    expect(product.getAttribute("property")).toBe("mainEntity");
    expect(product.parentElement!.getAttribute("vocab")).toBe("https://schema.org/");
    expect(product.querySelector("[property='price']")!.getAttribute("content")).toBe("229.00");
  });

  it("retains sibling article context alongside a recommendation scope", () => {
    const doc = buildDoc(
      "",
      `<article itemscope itemtype="https://schema.org/Article"><span itemprop="headline">Article about lamps</span></article>
       <aside itemscope itemtype="https://schema.org/Product"><span itemprop="name">Recommended lamp</span></aside>`,
    );
    const evidence = captureProductHtml(doc)!;
    expect(evidence).toContain('itemtype="https://schema.org/Article"');
    expect(evidence).toContain('itemtype="https://schema.org/Product"');
  });

  it("skips an oversized group of graphs instead of losing its mainEntity context", () => {
    const small = '<script type="application/ld+json">{"@type":"Product"}</script>';
    const large = `<script type="application/ld+json">{"@type":"WebPage","description":"${"💡".repeat(MAX_PRODUCT_HTML_BYTES / 4)}"}</script>`;
    const evidence = captureProductHtml(
      buildDoc(`<meta property="og:type" content="product">${small}${large}`),
    );
    expect(evidence).toBeUndefined();
  });

  it("omits deeply nested live scopes atomically before recursive serialization", () => {
    const doc = buildDoc(
      '<meta property="og:type" content="book">',
      `${'<div itemscope itemtype="https://schema.org/Book" itemprop="workExample">'.repeat(160)}<meta itemprop="name" content="Nested book">${"</div>".repeat(160)}`,
    );
    expect(captureProductHtml(doc)).toBeUndefined();
  });

  it("omits excessive DOM traversal without retaining an isolated recommended work", () => {
    const doc = buildDoc(
      '<meta property="og:type" content="book">',
      `<article itemscope itemtype="https://schema.org/Article">${"<span></span>".repeat(16_384)}</article>
       <aside itemscope itemtype="https://schema.org/Book"><span itemprop="name">Recommended book</span></aside>`,
    );
    expect(captureProductHtml(doc)).toBeUndefined();
  });

  it("does not invent product evidence from a visible price or a product-like URL", () => {
    expect(captureProductHtml(buildDoc("", "<p>Today's price: €229.00</p>"))).toBeUndefined();
  });
});
