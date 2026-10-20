// SPDX-License-Identifier: MIT
use oia_core::{parse_page_metadata, LinkCategory};
const URL: &str = "https://www.amazon.co.uk/Principles-Life-Work-Ray-Dalio-ebook/dp/B071RBPKGR/ref=tmm_kin_swatch_0";
const HTML: &str = include_str!("fixtures/amazon-book.html");

#[test]
fn amazon_book_keeps_main_title_author_cover_and_price() {
    let page = parse_page_metadata(HTML, URL);
    let book = page
        .link
        .expect("Amazon book must not remain a regular link");
    assert_eq!(book.category, LinkCategory::Book);
    assert_eq!(book.author.as_deref(), Some("Ray Dalio"));
    assert_eq!(page.title.as_deref(), Some("Principles: Life and Work"));
    assert!(page.image_url.unwrap().contains("61suprU8a5L"));
    let product = page.product.unwrap();
    assert_eq!(product.price.as_deref(), Some("23.76"));
    assert_eq!(product.currency.as_deref(), Some("EUR"));
}

#[test]
fn amazon_rejects_unrelated_routes_hosts_and_conflicting_identity() {
    for url in [
        "https://www.amazon.co.uk.evil.example/dp/B071RBPKGR",
        "https://www.amazon.co.uk/s?k=B071RBPKGR",
        "https://www.amazon.co.uk/dp/B071RBPKGR/reviews/extra",
        "https://www.amazon.co.uk/dp/B071RBPKGR/reviews",
        "https://www.amazon.co.uk/dp/B071RBPKG",
        "https://www.amazon.co.uk/dp/B000000000",
    ] {
        let page = parse_page_metadata(HTML, url);
        assert!(page.link.is_none(), "{url}");
        assert!(page.product.is_none(), "{url}");
    }
    assert!(
        parse_page_metadata(&format!("{HTML}<span id='productTitle'>Other</span>"), URL)
            .product
            .is_none()
    );
    assert!(parse_page_metadata("", URL).product.is_none());
}

#[test]
fn amazon_general_products_keep_product_presentation_without_becoming_books() {
    let html = HTML.replace("id=\"bylineInfo\"", "id=\"unrelated\"");
    let page = parse_page_metadata(&html, URL);
    assert!(page.link.is_none());
    assert!(page.product.is_some());
}

#[test]
fn amazon_unavailable_price_still_recognizes_book_without_inventing_a_price() {
    let page = parse_page_metadata(&HTML.replace("a-price-symbol", "unavailable"), URL);
    assert_eq!(page.link.unwrap().category, LinkCategory::Book);
    assert!(page.product.unwrap().price.is_none());
}
