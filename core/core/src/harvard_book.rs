// SPDX-License-Identifier: MIT

//! Harvard's main book uses purchase attributes; its Book scopes are recommendations.

use scraper::{Html, Selector};
use url::Url;

use crate::link_metadata::{normalize_isbn, DetectedLinkMetadata, LinkCategory};

pub(crate) fn inspect(document: &Html, page: &Url) -> Result<Option<DetectedLinkMetadata>, ()> {
    if !matches!(
        page.host_str(),
        Some("hup.harvard.edu" | "www.hup.harvard.edu")
    ) {
        return Ok(None);
    }
    let parts: Vec<_> = page.path().trim_end_matches('/').split('/').collect();
    let ["", "books", raw_isbn] = parts.as_slice() else {
        return Ok(None);
    };
    if raw_isbn.len() != 13 || !raw_isbn.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    let isbn = normalize_isbn(raw_isbn).ok_or(())?;
    let selector = Selector::parse("[data-component='heros:hero-product']").unwrap();
    let headers: Vec<_> = document.select(&selector).take(2).collect();
    let [header] = headers.as_slice() else {
        return Ok(None);
    };
    let selector = Selector::parse("[data-purchase-isbn13]").unwrap();
    let purchases: Vec<_> = header.select(&selector).take(2).collect();
    let [purchase] = purchases.as_slice() else {
        return Err(());
    };
    if purchase.value().attr("data-purchase-isbn13") != Some(isbn.as_str()) {
        return Err(());
    }
    let title = purchase
        .value()
        .attr("data-purchase-title")
        .ok_or(())?
        .trim();
    let headings = Selector::parse("h1").unwrap();
    let titles: Vec<_> = header.select(&headings).take(2).collect();
    if title.is_empty()
        || title.len() > 2048
        || titles.len() != 1
        || titles[0].text().collect::<String>().trim() != title
    {
        return Err(());
    }
    let authors = Selector::parse("a[href]").unwrap();
    let author_names: Vec<_> = header
        .select(&authors)
        .filter(|element| {
            element
                .value()
                .attr("href")
                .and_then(|href| page.join(href).ok())
                .is_some_and(|url| {
                    matches!(
                        url.host_str(),
                        Some("hup.harvard.edu" | "www.hup.harvard.edu")
                    ) && url.path().starts_with("/authors/")
                        && url.path().len() > "/authors/".len()
                })
        })
        .map(|element| element.text().collect::<String>().trim().to_string())
        .filter(|name| !name.is_empty() && name.len() <= 2048)
        .take(17)
        .collect();
    if author_names.len() > 16 {
        return Err(());
    }
    let images = Selector::parse("meta[property='og:image']").unwrap();
    let image_url = document
        .select(&images)
        .filter_map(|element| element.value().attr("content"))
        .filter_map(|raw| page.join(raw).ok())
        .find(crate::product::is_public_metadata_url)
        .map(|url| url.to_string());
    Ok(Some(DetectedLinkMetadata {
        category: LinkCategory::Book,
        title: Some(title.to_string()),
        isbn: Some(isbn),
        image_url,
        author: (!author_names.is_empty()).then(|| author_names.join(", ")),
    }))
}
