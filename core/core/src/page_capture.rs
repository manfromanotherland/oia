// SPDX-License-Identifier: MIT

//! The browser extracts content; the core decides whether that content is an article.

use std::collections::{HashMap, HashSet};

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::product::{
    collect_nodes, has_type, resolved, structured_dom_products, structured_json_roots, url_score,
    MAX_BROWSER_PRODUCT_HTML_BYTES, MAX_STRUCTURED_NODES,
};

pub const MAX_PAGE_CAPTURE_PARAGRAPHS: usize = 1024;

/// Raw character measurements from the extracted HTML, before Markdown conversion.
/// These observations do not prescribe a card kind or a classification threshold.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageCaptureEvidence {
    pub text_characters: u32,
    pub link_characters: u32,
    pub paragraph_characters: Vec<u32>,
}

impl PageCaptureEvidence {
    pub fn is_valid(&self) -> bool {
        self.paragraph_characters.len() <= MAX_PAGE_CAPTURE_PARAGRAPHS
            && self.link_characters <= self.text_characters
            && self
                .paragraph_characters
                .iter()
                .all(|length| *length <= self.text_characters)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PageCaptureKind {
    Article,
    Link,
}

/// Prefer a durable link when extraction is absent or ambiguous. Explicit main
/// article metadata permits short articles; untyped pages need sustained prose.
/// Product and cultural catalog pages keep their specialized link presentation.
pub fn classify_page_capture(
    page_url: &str,
    structured_html: &str,
    markdown: &str,
    evidence: Option<&PageCaptureEvidence>,
) -> PageCaptureKind {
    if markdown.trim().is_empty() {
        return PageCaptureKind::Link;
    }
    if structured_html.len() > MAX_BROWSER_PRODUCT_HTML_BYTES {
        return PageCaptureKind::Link;
    }
    match page_metadata_context(structured_html, page_url) {
        PageMetadataContext::Article => return PageCaptureKind::Article,
        PageMetadataContext::Link => return PageCaptureKind::Link,
        PageMetadataContext::Undeclared => {}
    }
    if crate::parse_product_metadata(structured_html, page_url).is_some()
        || crate::parse_link_metadata(structured_html, page_url).is_some()
    {
        return PageCaptureKind::Link;
    }
    let Some(evidence) = evidence.filter(|evidence| evidence.is_valid()) else {
        return PageCaptureKind::Link;
    };
    // Landing pages may extract successfully while consisting of labels and links.
    let prose: Vec<_> = evidence
        .paragraph_characters
        .iter()
        .copied()
        .filter(|characters| *characters >= 120)
        .collect();
    let prose_characters: u64 = prose.iter().map(|characters| u64::from(*characters)).sum();
    let sustained_prose = (prose.len() >= 2 && prose_characters >= 500)
        || prose.iter().any(|characters| *characters >= 700);
    let text = u64::from(evidence.text_characters);
    let links = u64::from(evidence.link_characters);
    if sustained_prose && text >= 500 && links * 3 <= text && prose_characters * 2 >= text {
        PageCaptureKind::Article
    } else {
        PageCaptureKind::Link
    }
}

enum PageMetadataContext {
    Article,
    Link,
    Undeclared,
}

fn page_metadata_context(html: &str, page_url: &str) -> PageMetadataContext {
    let Ok(page) = Url::parse(page_url) else {
        return PageMetadataContext::Undeclared;
    };
    let document = Html::parse_document(html);
    let metas = Selector::parse("meta[property], meta[name]").expect("static selector");
    let og: HashMap<_, _> = document
        .select(&metas)
        .filter_map(|element| {
            let name = element
                .value()
                .attr("property")
                .or_else(|| element.value().attr("name"))?;
            Some((name.to_ascii_lowercase(), element.value().attr("content")?))
        })
        .collect();
    if og
        .get("og:type")
        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("article"))
        && og
            .get("og:url")
            .is_none_or(|url| url_score(&serde_json::json!({ "url": url }), &page) > 0)
    {
        return PageMetadataContext::Article;
    }
    let Some(mut roots) = structured_json_roots(&document) else {
        return PageMetadataContext::Link;
    };
    let Some(dom_roots) = structured_dom_products(&document) else {
        return PageMetadataContext::Link;
    };
    roots.extend(dom_roots);
    let mut nodes = Vec::new();
    for root in &roots {
        collect_nodes(root, true, &mut nodes);
    }
    if nodes.len() > MAX_STRUCTURED_NODES {
        return PageMetadataContext::Link;
    }
    let ids: HashMap<_, _> = nodes
        .iter()
        .filter_map(|(node, _)| {
            (node.as_object()?.len() > 1).then_some((node.get("@id")?.as_str()?, *node))
        })
        .collect();
    let blocked_ids: HashSet<_> = nodes
        .iter()
        .filter(|(_, eligible)| !eligible)
        .filter_map(|(node, _)| node.get("@id").and_then(Value::as_str))
        .collect();
    let is_unblocked = |node: &Value| {
        !node
            .get("@id")
            .and_then(Value::as_str)
            .is_some_and(|id| blocked_ids.contains(id))
    };
    if nodes.iter().any(|(node, eligible)| {
        *eligible
            && has_type(node, "WebPage")
            && url_score(node, &page) > 0
            && node.get("mainEntity").is_some_and(|main| {
                let main = resolved(main, &ids);
                is_article(main) && is_unblocked(main)
            })
    }) {
        return PageMetadataContext::Article;
    }
    let articles: Vec<_> = nodes
        .iter()
        .filter(|(node, eligible)| *eligible && is_article(node) && is_unblocked(node))
        .map(|(node, _)| *node)
        .collect();
    if articles.iter().any(|node| url_score(node, &page) > 0) {
        return PageMetadataContext::Article;
    }
    // One root Article without its own identity is conventional page metadata.
    // A collection, catalog entity, or another identified page removes that inference.
    let unscoped_article = articles.len() == 1
        && ["url", "@id", "mainEntityOfPage"]
            .iter()
            .all(|key| articles[0].get(*key).is_none())
        && !nodes.iter().any(|(node, eligible)| {
            *eligible
                && (has_type(node, "ItemList")
                    || has_type(node, "CollectionPage")
                    || has_type(node, "Product")
                    || has_type(node, "ProductGroup")
                    || has_type(node, "Book")
                    || has_type(node, "Movie")
                    || has_type(node, "MusicAlbum")
                    || (has_type(node, "WebPage")
                        && ["url", "@id"].iter().any(|key| node.get(*key).is_some())
                        && url_score(node, &page) == 0))
        });
    if unscoped_article {
        return PageMetadataContext::Article;
    }
    let declared_link = og.get("og:type").is_some_and(|kind| {
        matches!(
            kind.trim().to_ascii_lowercase().as_str(),
            "profile" | "video.tv_show" | "video.episode"
        )
    });
    let collection = nodes.iter().any(|(node, eligible)| {
        *eligible
            && [
                "CollectionPage",
                "SearchResultsPage",
                "ProfilePage",
                "ItemList",
            ]
            .iter()
            .any(|kind| has_type(node, kind))
            && (url_score(node, &page) > 0
                || ["url", "@id"].iter().all(|key| node.get(*key).is_none()))
    });
    if declared_link || collection {
        PageMetadataContext::Link
    } else {
        PageMetadataContext::Undeclared
    }
}

fn is_article(node: &Value) -> bool {
    [
        "Article",
        "BlogPosting",
        "NewsArticle",
        "TechArticle",
        "ScholarlyArticle",
        "Report",
    ]
    .iter()
    .any(|kind| has_type(node, kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://example.com/page";
    const BODY: &str = "Captured content from the live browser.";

    fn prose() -> PageCaptureEvidence {
        PageCaptureEvidence {
            text_characters: 1000,
            link_characters: 40,
            paragraph_characters: vec![450, 450],
        }
    }

    #[test]
    fn extraction_failure_and_landing_page_labels_become_links() {
        assert_eq!(
            classify_page_capture(URL, "", "", Some(&prose())),
            PageCaptureKind::Link
        );
        let labels = PageCaptureEvidence {
            text_characters: 1200,
            link_characters: 120,
            paragraph_characters: vec![40; 30],
        };
        assert_eq!(
            classify_page_capture(URL, "", BODY, Some(&labels)),
            PageCaptureKind::Link
        );
    }

    #[test]
    fn untyped_prose_is_an_article_but_link_heavy_collections_are_links() {
        assert_eq!(
            classify_page_capture(URL, "", BODY, Some(&prose())),
            PageCaptureKind::Article
        );
        let mut linked = prose();
        linked.link_characters = 650;
        assert_eq!(
            classify_page_capture(URL, "", BODY, Some(&linked)),
            PageCaptureKind::Link
        );
    }

    #[test]
    fn main_article_metadata_supports_short_bodies_and_ignores_recommendations() {
        for html in [
            r#"<meta property="og:type" content="article">"#,
            r#"<script type="application/ld+json">{"@type":"NewsArticle","url":"https://example.com/page"}</script>"#,
            r##"<script type="application/ld+json">{"@graph":[{"@type":"WebPage","url":"https://example.com/page","mainEntity":{"@id":"#article"}},{"@id":"#article","@type":"BlogPosting"}]}</script>"##,
            r#"<main itemscope itemtype="https://schema.org/Article"><link itemprop="url" href="https://example.com/page"></main>"#,
        ] {
            assert_eq!(
                classify_page_capture(URL, html, BODY, None),
                PageCaptureKind::Article,
                "{html}"
            );
        }
        for html in [
            r#"<meta property="og:type" content="article"><meta property="og:url" content="https://example.com/other">"#,
            r#"<script type="application/ld+json">{"@type":"NewsArticle","url":"https://example.com/other"}</script>"#,
            r#"<script type="application/ld+json">{"@type":"CollectionPage","itemListElement":[{"@type":"Article","url":"https://example.com/page"}]}</script>"#,
        ] {
            assert_eq!(
                classify_page_capture(URL, html, BODY, None),
                PageCaptureKind::Link,
                "{html}"
            );
        }
    }

    #[test]
    fn catalog_entities_use_links_despite_long_extracted_descriptions() {
        for kind in ["Product", "Book", "MusicAlbum", "Movie"] {
            let html = format!(
                r#"<script type="application/ld+json">{{"@type":"{kind}","url":"{URL}"}}</script>"#
            );
            assert_eq!(
                classify_page_capture(URL, &html, BODY, Some(&prose())),
                PageCaptureKind::Link,
                "{kind}"
            );
        }
        assert_eq!(
            classify_page_capture(
                "https://letterboxd.com/film/the-matrix/",
                "",
                BODY,
                Some(&prose())
            ),
            PageCaptureKind::Link
        );
    }

    #[test]
    fn matching_article_context_wins_over_a_mentioned_product() {
        let html = r#"<script type="application/ld+json">{"@type":"Article","url":"https://example.com/page","mentions":{"@type":"Product","name":"A lamp"}}</script>"#;
        assert_eq!(
            classify_page_capture(URL, html, BODY, None),
            PageCaptureKind::Article
        );
    }

    #[test]
    fn owned_offer_identity_keeps_duplicate_product_graphs_out_of_articles() {
        let url = "https://bo-ha.com/en-ie/products/ronja-hand-painted-ceramic-pendant-light?variant=50744649023620&ad_id=120249781090550484";
        let html = r#"<meta property="og:type" content="website"><script type="application/ld+json">{"@type":"Product","name":"Ronja","url":"https://bo-ha.com/en-ie/en-ie/products/ronja-hand-painted-ceramic-pendant-light"}</script><script type="application/ld+json">{"@type":"Product","name":"Ronja","offers":{"@type":"Offer","url":"https://bo-ha.com/en-ie/products/ronja-hand-painted-ceramic-pendant-light?variant=50744649023620","price":149.95,"priceCurrency":"USD"}}</script>"#;
        assert_eq!(
            classify_page_capture(url, html, BODY, Some(&prose())),
            PageCaptureKind::Link
        );
        let product = crate::parse_product_metadata(html, url).unwrap();
        assert_eq!(product.price.as_deref(), Some("149.95"));
        assert_eq!(product.currency.as_deref(), Some("USD"));
    }

    #[test]
    fn collection_context_keeps_long_item_descriptions_as_links() {
        for kind in [
            "CollectionPage",
            "SearchResultsPage",
            "ProfilePage",
            "ItemList",
        ] {
            let html = format!(
                r#"<script type="application/ld+json">{{"@type":"{kind}","url":"{URL}"}}</script>"#
            );
            assert_eq!(
                classify_page_capture(URL, &html, BODY, Some(&prose())),
                PageCaptureKind::Link,
                "{kind}"
            );
        }
    }

    #[test]
    fn bounded_numeric_evidence_uses_wide_arithmetic() {
        let mut invalid = prose();
        invalid.paragraph_characters = vec![500; MAX_PAGE_CAPTURE_PARAGRAPHS + 1];
        assert!(!invalid.is_valid());
        assert_eq!(
            classify_page_capture(URL, "", BODY, Some(&invalid)),
            PageCaptureKind::Link
        );
        let large = PageCaptureEvidence {
            text_characters: u32::MAX,
            link_characters: u32::MAX,
            paragraph_characters: vec![u32::MAX; MAX_PAGE_CAPTURE_PARAGRAPHS],
        };
        assert_eq!(
            classify_page_capture(URL, "", BODY, Some(&large)),
            PageCaptureKind::Link
        );
    }
}
