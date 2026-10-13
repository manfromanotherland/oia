// SPDX-License-Identifier: MIT
use oia_core::{
    classify_page_capture, parse_link_metadata, LinkCategory, PageCaptureEvidence, PageCaptureKind,
};
const URL: &str = "https://www.hup.harvard.edu/books/9780674627512";
const HTML: &str = include_str!("fixtures/harvard-book.html");

#[test]
fn harvard_main_book_wins_over_recommendations() {
    let book = parse_link_metadata(HTML, URL).expect("Harvard main book should be recognized");
    assert_eq!(book.category, LinkCategory::Book);
    assert_eq!(
        book.title.as_deref(),
        Some("Notes on the Synthesis of Form")
    );
    assert_eq!(book.author.as_deref(), Some("Christopher Alexander"));
    assert_eq!(book.isbn.as_deref(), Some("9780674627512"));
    assert!(book.image_url.unwrap().contains("9780674627512.png"));
    assert_eq!(
        classify_page_capture(
            URL,
            HTML,
            "Long book description",
            Some(&PageCaptureEvidence {
                text_characters: 1000,
                link_characters: 0,
                paragraph_characters: vec![500, 500],
            })
        ),
        PageCaptureKind::Link
    );
}

#[test]
fn harvard_adapter_rejects_unrelated_or_conflicting_evidence() {
    for (html, url) in [
        (
            HTML.to_string(),
            "https://www.hup.harvard.edu.evil.example/books/9780674627512",
        ),
        (
            HTML.to_string(),
            "https://www.hup.harvard.edu/books/9780674627513",
        ),
        (
            HTML.to_string(),
            "https://www.hup.harvard.edu/books/9780674627512/review",
        ),
        (
            HTML.replace(
                "data-purchase-isbn13=\"9780674627512\"",
                "data-purchase-isbn13=\"9780674304536\"",
            ),
            URL,
        ),
        (
            HTML.replace(
                "data-purchase-title=\"Notes on the Synthesis of Form\"",
                "data-purchase-title=\"Another book\"",
            ),
            URL,
        ),
        (HTML.replace("heros:hero-product", "unrelated"), URL),
        (
            HTML.replace("content=\"website\"", "content=\"article\""),
            URL,
        ),
        (
            HTML.replace(
                "</body>",
                "<header data-component='heros:hero-product'></header></body>",
            ),
            URL,
        ),
    ] {
        assert!(parse_link_metadata(&html, url).is_none(), "{url}");
    }
    assert!(parse_link_metadata("", URL).is_none());
}

#[test]
fn normalized_harvard_url_keeps_main_author() {
    let book = parse_link_metadata(HTML, "https://hup.harvard.edu/books/9780674627512").unwrap();
    assert_eq!(book.author.as_deref(), Some("Christopher Alexander"));
}
