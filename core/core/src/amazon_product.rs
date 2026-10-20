// SPDX-License-Identifier: MIT

//! Amazon exposes its main product in DOM fields rather than schema.org metadata.
use scraper::{ElementRef, Html, Selector};
use url::Url;

use crate::{
    link_metadata::{DetectedLinkMetadata, LinkCategory},
    product::{currency_code, decimal_price, is_public_metadata_url, DetectedProductMetadata},
    ProductMetadata,
};

fn asin(page: &Url) -> Option<&str> {
    let host = page
        .host_str()?
        .strip_prefix("www.")
        .unwrap_or(page.host_str()?);
    if !matches!(
        host,
        "amazon.co.uk"
            | "amazon.com"
            | "amazon.de"
            | "amazon.fr"
            | "amazon.it"
            | "amazon.es"
            | "amazon.ca"
            | "amazon.com.au"
            | "amazon.co.jp"
    ) {
        return None;
    }
    let parts: Vec<_> = page.path().trim_matches('/').split('/').collect();
    let id = match parts.as_slice() {
        ["dp", id] | [_, "dp", id] | ["gp", "product", id] => *id,
        ["dp", id, reference] | [_, "dp", id, reference] if reference.starts_with("ref=") => *id,
        _ => return None,
    };
    (id.len() == 10
        && id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
    .then_some(id)
}

fn one<'a>(document: &'a Html, selector: &str) -> Option<ElementRef<'a>> {
    let selector = Selector::parse(selector).ok()?;
    let mut elements = document.select(&selector);
    let first = elements.next()?;
    elements.next().is_none().then_some(first)
}

fn text(element: ElementRef<'_>) -> Option<String> {
    let value = element
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!value.is_empty() && value.len() <= 2048).then_some(value)
}

pub(crate) fn inspect(
    document: &Html,
    page: &Url,
) -> Option<(DetectedProductMetadata, Option<DetectedLinkMetadata>)> {
    let id = asin(page)?;
    if !is_public_metadata_url(page) {
        return None;
    }
    if let Some(canonical) = one(document, "link[rel='canonical']") {
        let canonical = page.join(canonical.value().attr("href")?).ok()?;
        if canonical.origin() != page.origin() || asin(&canonical) != Some(id) {
            return None;
        }
    }
    let title = text(one(document, "#productTitle, #ebooksProductTitle")?)?;
    let image_url = one(document, "#landingImage, #imgBlkFront")
        .and_then(|image| page.join(image.value().attr("src")?).ok())
        .filter(is_public_metadata_url)
        .map(|url| url.to_string());
    let author_selector = Selector::parse("#bylineInfo .author a").unwrap();
    let authors: Vec<_> = document
        .select(&author_selector)
        .take(17)
        .filter_map(text)
        .collect();
    let author = (!authors.is_empty() && authors.len() <= 16).then(|| authors.join(", "));
    // A main author byline establishes a book; a retail route alone does not.
    let link = author.map(|author| DetectedLinkMetadata {
        category: LinkCategory::Book,
        title: Some(title.clone()),
        image_url: image_url.clone(),
        isbn: None,
        author: Some(author),
    });
    let price = one(document, "#priceBlock-outsideOfForm_feature_div .apex-pricetopay-value, #corePrice_feature_div .a-price, #corePriceDisplay_desktop_feature_div .priceToPay")
        .and_then(|price| {
            let select = |selector: &str| price.select(&Selector::parse(selector).ok()?).next().and_then(text);
            let whole = select(".a-price-whole")?.trim_end_matches(['.', ',']).replace([',', ' '], "");
            let fraction = select(".a-price-fraction").unwrap_or_default();
            let amount = decimal_price(&if fraction.is_empty() { whole } else { format!("{whole}.{fraction}") })?;
            let symbol = select(".a-price-symbol")?;
            let currency = match symbol.as_str() { "£" => "GBP".into(), "€" => "EUR".into(), _ => currency_code(&symbol)? };
            Some((amount, currency))
        });
    let (price, currency) = price.map(|(p, c)| (Some(p), Some(c))).unwrap_or_default();
    Some((
        DetectedProductMetadata {
            product: ProductMetadata {
                version: 1,
                price,
                currency,
                price_is_from: false,
                captured_at: Some(crate::time::now_utc_iso()),
            },
            title: Some(title),
            image_url,
        },
        link,
    ))
}
