// SPDX-License-Identifier: MIT

//! Conservative, provider-neutral product metadata. A product remains an
//! article or lightweight link; this durable snapshot only changes presentation.

use crate::{LibraryRoot, ProductMetadata, ReadingKind};
use anyhow::{anyhow, bail, Result};
use reqwest::{
    blocking::Client,
    header::{ACCEPT, CONTENT_TYPE, LOCATION},
    redirect::Policy,
};
use scraper::{ElementRef, Html, Selector};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    net::{IpAddr, ToSocketAddrs},
    time::{Duration, Instant},
};
use url::Url;

pub const MAX_PRODUCT_HTML_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_BROWSER_PRODUCT_HTML_BYTES: usize = 512 * 1024;
const FETCH_DEADLINE: Duration = Duration::from_secs(8);

/// Identify the main product using JSON-LD, scoped schema.org markup, or explicit
/// product Open Graph metadata. Bare amounts and commerce-looking URLs never
/// establish product identity. Missing or invalid prices do not hide a product.
pub fn parse_product_metadata(html: &str, page_url: &str) -> Option<ProductMetadata> {
    if html.len() > MAX_PRODUCT_HTML_BYTES {
        return None;
    }
    let page = Url::parse(page_url).ok()?;
    if !matches!(page.scheme(), "http" | "https") {
        return None;
    }
    let document = Html::parse_document(html);
    let scripts = Selector::parse("script[type]").ok()?;
    let roots: Vec<Value> = document
        .select(&scripts)
        .filter(|element| {
            element.value().attr("type").is_some_and(|mime| {
                mime.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("application/ld+json")
            })
        })
        .filter_map(|element| serde_json::from_str(&element.inner_html()).ok())
        .collect();
    let mut nodes = Vec::new();
    for root in &roots {
        collect_nodes(root, true, &mut nodes);
    }
    let ids: HashMap<&str, &Value> = nodes
        .iter()
        .filter_map(|(node, _)| {
            if node.as_object()?.len() <= 1 {
                return None;
            }
            Some((node.get("@id")?.as_str()?, *node))
        })
        .collect();
    if let Some(product) = choose_product(&nodes, &ids, &page) {
        return Some(with_og_price(metadata_for(product, &ids, &page), &document));
    }
    let scoped = structured_dom_products(&document);
    let mut dom_nodes = Vec::new();
    for root in &scoped {
        collect_nodes(root, true, &mut dom_nodes);
    }
    if negative_page_context(&nodes) {
        return None;
    }
    if let Some(product) = choose_product(&dom_nodes, &HashMap::new(), &page) {
        return Some(with_og_price(
            metadata_for(product, &HashMap::new(), &page),
            &document,
        ));
    }
    open_graph_product(&document)
}

fn with_og_price(mut product: ProductMetadata, document: &Html) -> ProductMetadata {
    if product.price.is_none() {
        if let Some(og) = open_graph_product(document).filter(|og| og.price.is_some()) {
            product.price = og.price;
            product.currency = og.currency;
            product.price_is_from = og.price_is_from;
        }
    }
    product
}

fn schema_name(value: &str) -> &str {
    if value.contains("://")
        && !value.starts_with("https://schema.org/")
        && !value.starts_with("http://schema.org/")
    {
        return "";
    }
    value.rsplit(['/', '#', ':']).next().unwrap_or(value)
}
fn has_type(value: &Value, kind: &str) -> bool {
    value.get("@type").is_some_and(|value| match value {
        Value::String(name) => schema_name(name) == kind,
        Value::Array(names) => names
            .iter()
            .any(|name| name.as_str().is_some_and(|name| schema_name(name) == kind)),
        _ => false,
    })
}
fn is_product(value: &Value) -> bool {
    has_type(value, "Product") || has_type(value, "ProductGroup")
}
fn collect_nodes<'a>(value: &'a Value, eligible: bool, nodes: &mut Vec<(&'a Value, bool)>) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_nodes(value, eligible, nodes);
            }
        }
        Value::Object(object) => {
            nodes.push((value, eligible));
            for (key, child) in object {
                let blocked = matches!(
                    key.as_str(),
                    "itemReviewed"
                        | "review"
                        | "reviews"
                        | "mentions"
                        | "isRelatedTo"
                        | "isSimilarTo"
                        | "itemListElement"
                        | "item"
                        | "subjectOf"
                        | "recommendations"
                        | "relatedProducts"
                );
                collect_nodes(child, eligible && !blocked, nodes);
            }
        }
        _ => {}
    }
}
fn resolved<'a>(value: &'a Value, ids: &HashMap<&str, &'a Value>) -> &'a Value {
    if value.as_object().is_some_and(|o| o.len() == 1) {
        if let Some(node) = value
            .get("@id")
            .and_then(Value::as_str)
            .and_then(|id| ids.get(id))
        {
            return node;
        }
    }
    value
}
fn url_score(value: &Value, page: &Url) -> u8 {
    let mut score = 0;
    for key in ["url", "@id", "mainEntityOfPage"] {
        let raw = value
            .get(key)
            .and_then(|v| v.as_str().or_else(|| v.get("@id").and_then(Value::as_str)));
        if let Some(url) = raw
            .filter(|raw| !raw.starts_with('#'))
            .and_then(|raw| page.join(raw).ok())
        {
            let mut url = url;
            url.set_fragment(None);
            let mut target = page.clone();
            target.set_fragment(None);
            if crate::normalize_url(url.as_str()).ok() == crate::normalize_url(target.as_str()).ok()
            {
                score = score.max(3);
            } else if url.origin() == target.origin()
                && url.path().trim_end_matches('/') == target.path().trim_end_matches('/')
            {
                score = score.max(2);
            }
        }
    }
    score
}
fn negative_page_context(nodes: &[(&Value, bool)]) -> bool {
    nodes.iter().any(|(node, allowed)| {
        *allowed
            && (has_type(node, "ItemList")
                || has_type(node, "Review")
                || has_type(node, "Article")
                || has_type(node, "BlogPosting")
                || has_type(node, "CollectionPage"))
    })
}
fn choose_product<'a>(
    nodes: &[(&'a Value, bool)],
    ids: &HashMap<&str, &'a Value>,
    page: &Url,
) -> Option<&'a Value> {
    let explicit = nodes
        .iter()
        .filter(|(node, allowed)| {
            *allowed
                && has_type(node, "WebPage")
                && (url_score(node, page) > 0
                    || (node.get("url").is_none() && node.get("@id").is_none()))
        })
        .filter_map(|(node, _)| node.get("mainEntity"))
        .map(|node| resolved(node, ids))
        .find(|node| is_product(node));
    if explicit.is_some() {
        return explicit;
    }
    if negative_page_context(nodes) {
        return None;
    }
    let foreign_entities: Vec<_> = nodes
        .iter()
        .filter(|(node, _)| {
            has_type(node, "WebPage")
                && (node.get("url").is_some() || node.get("@id").is_some())
                && url_score(node, page) == 0
        })
        .filter_map(|(node, _)| node.get("mainEntity"))
        .map(|node| resolved(node, ids))
        .collect();
    let products: Vec<_> = nodes
        .iter()
        .filter(|(node, allowed)| *allowed && is_product(node) && !foreign_entities.contains(node))
        .map(|(node, _)| *node)
        .collect();
    let best = products
        .iter()
        .map(|node| (url_score(node, page), *node))
        .filter(|(score, _)| *score > 0)
        .max_by_key(|(score, _)| *score);
    if let Some((best_score, best)) = best {
        let matching = products
            .iter()
            .filter(|node| url_score(node, page) == best_score)
            .count();
        if matching == 1 {
            return Some(best);
        }
    }
    // A Review, Article, or collection is never classified because it happens
    // to mention a single Product in otherwise unrelated structured data.
    let roots: Vec<_> = products
        .iter()
        .filter(|candidate| {
            !products.iter().any(|parent| {
                parent.get("hasVariant").is_some_and(|variants| {
                    array_values(variants)
                        .iter()
                        .any(|variant| contains_node(resolved(variant, ids), candidate))
                })
            })
        })
        .copied()
        .collect();
    (roots.len() == 1 && (roots[0].get("url").is_none() || url_score(roots[0], page) > 0))
        .then(|| roots[0])
}
fn contains_node(parent: &Value, node: &Value) -> bool {
    parent == node
        || parent
            .as_array()
            .is_some_and(|values| values.iter().any(|v| contains_node(v, node)))
}
fn metadata_for(product: &Value, ids: &HashMap<&str, &Value>, page: &Url) -> ProductMetadata {
    let selected_variant = product
        .get("hasVariant")
        .into_iter()
        .flat_map(array_values)
        .map(|v| resolved(v, ids))
        .find(|v| url_score(v, page) == 3);
    let selected = selected_variant.unwrap_or(product);
    let mut offers: Vec<_> = selected
        .get("offers")
        .into_iter()
        .flat_map(array_values)
        .map(|v| resolved(v, ids))
        .collect();
    let variant_range =
        selected_variant.is_none() && offers.is_empty() && product.get("hasVariant").is_some();
    if variant_range {
        offers = product
            .get("hasVariant")
            .into_iter()
            .flat_map(array_values)
            .map(|v| resolved(v, ids))
            .filter_map(|variant| variant.get("offers"))
            .flat_map(array_values)
            .map(|offer| resolved(offer, ids))
            .collect();
    }
    let matched: Vec<_> = offers
        .iter()
        .copied()
        .filter(|offer| url_score(offer, page) == 3)
        .collect();
    let candidates = if matched.is_empty() { offers } else { matched };
    let mut prices: Vec<(String, String, bool)> = candidates
        .iter()
        .filter_map(|offer| offer_price(offer, ids))
        .collect();
    let currency = prices.first().map(|v| v.1.clone());
    // Combining offers in different currencies would silently invent a price.
    if prices.iter().any(|v| Some(&v.1) != currency.as_ref()) {
        prices.clear();
    }
    prices.sort_by(|a, b| decimal_cmp(&a.0, &b.0));
    let range = prices
        .iter()
        .any(|v| prices.first().is_some_and(|first| v.0 != first.0));
    let (price, currency, price_is_from) = prices
        .first()
        .map(|v| {
            (
                Some(v.0.clone()),
                Some(v.1.clone()),
                v.2 || range || variant_range,
            )
        })
        .unwrap_or((None, None, false));
    ProductMetadata {
        version: 1,
        price,
        currency,
        price_is_from,
        captured_at: Some(crate::time::now_utc_iso()),
    }
}
fn array_values(value: &Value) -> Vec<&Value> {
    value
        .as_array()
        .map(|v| v.iter().collect())
        .unwrap_or_else(|| vec![value])
}
fn offer_price(offer: &Value, ids: &HashMap<&str, &Value>) -> Option<(String, String, bool)> {
    if let Some(expiry) = offer.get("priceValidUntil").and_then(Value::as_str) {
        if let Some(date) = expiry.get(..10).filter(|date| {
            date.bytes().enumerate().all(|(i, b)| {
                if i == 4 || i == 7 {
                    b == b'-'
                } else {
                    b.is_ascii_digit()
                }
            })
        }) {
            if date < &crate::time::now_utc_iso()[..10] {
                return None;
            }
        }
    }
    let aggregate = has_type(offer, "AggregateOffer");
    let price_key = if aggregate { "lowPrice" } else { "price" };
    let direct = offer.get(price_key).and_then(price_value).zip(
        offer
            .get("priceCurrency")
            .and_then(Value::as_str)
            .and_then(currency_code),
    );
    if let Some((price, currency)) = direct {
        return Some((price, currency, aggregate));
    }
    for spec in offer
        .get("priceSpecification")
        .into_iter()
        .flat_map(array_values)
    {
        let spec = resolved(spec, ids);
        if spec
            .get("priceType")
            .and_then(Value::as_str)
            .is_some_and(|t| schema_name(t) == "StrikethroughPrice")
        {
            continue;
        }
        let price = spec.get("price").and_then(price_value);
        let currency = spec
            .get("priceCurrency")
            .or_else(|| offer.get("priceCurrency"))
            .and_then(Value::as_str)
            .and_then(currency_code);
        if let Some((price, currency)) = price.zip(currency) {
            return Some((price, currency, aggregate));
        }
    }
    None
}
fn price_value(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => decimal_price(s),
        Value::Number(n) => decimal_price(&n.to_string()),
        _ => None,
    }
}
fn decimal_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (ai, af) = a.split_once('.').unwrap_or((a, ""));
    let (bi, bf) = b.split_once('.').unwrap_or((b, ""));
    ai.len()
        .cmp(&bi.len())
        .then_with(|| ai.cmp(bi))
        .then_with(|| format!("{af:0<20}").cmp(&format!("{bf:0<20}")))
}
pub fn decimal_price(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.len() > 40 || raw.is_empty() {
        return None;
    }
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 12
    {
        return None;
    }
    if raw.contains('.') && fraction.is_empty() {
        return None;
    }
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    Some(if fraction.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{fraction}")
    })
}
pub fn currency_code(raw: &str) -> Option<String> {
    let code = raw.trim().to_ascii_uppercase();
    const CODES:&str="AED AFN ALL AMD ANG AOA ARS AUD AWG AZN BAM BBD BDT BGN BHD BIF BMD BND BOB BRL BSD BTN BWP BYN BZD CAD CDF CHF CLP CNY COP CRC CUP CVE CZK DJF DKK DOP DZD EGP ERN ETB EUR FJD FKP GBP GEL GHS GIP GMD GNF GTQ GYD HKD HNL HTG HUF IDR ILS INR IQD IRR ISK JMD JOD JPY KES KGS KHR KMF KPW KRW KWD KYD KZT LAK LBP LKR LRD LSL LYD MAD MDL MGA MKD MMK MNT MOP MRU MUR MVR MWK MXN MYR MZN NAD NGN NIO NOK NPR NZD OMR PAB PEN PGK PHP PKR PLN PYG QAR RON RSD RUB RWF SAR SBD SCR SDG SEK SGD SHP SLE SLL SOS SRD SSP STN SVC SYP SZL THB TJS TMT TND TOP TRY TTD TWD TZS UAH UGX USD UYU UZS VED VES VND VUV WST XAF XCD XOF XPF YER ZAR ZMW ZWG";
    CODES.split_whitespace().any(|c| c == code).then_some(code)
}

/// Validate a product supplied by another ingress adapter before storing it.
pub(crate) fn validate_product(product: ProductMetadata) -> Option<ProductMetadata> {
    if product.version != 1 {
        return None;
    }
    let pair = product
        .price
        .as_deref()
        .and_then(decimal_price)
        .zip(product.currency.as_deref().and_then(currency_code));
    let (price, currency) = pair
        .map(|(p, c)| (Some(p), Some(c)))
        .unwrap_or((None, None));
    Some(ProductMetadata {
        price_is_from: product.price_is_from && price.is_some(),
        price,
        currency,
        ..product
    })
}

fn type_tokens(element: ElementRef<'_>) -> Vec<&str> {
    element
        .value()
        .attr("itemtype")
        .or_else(|| element.value().attr("typeof"))
        .unwrap_or("")
        .split_whitespace()
        .map(schema_name)
        .collect()
}
fn property_tokens(element: ElementRef<'_>) -> Vec<&str> {
    element
        .value()
        .attr("itemprop")
        .or_else(|| element.value().attr("property"))
        .unwrap_or("")
        .split_whitespace()
        .map(schema_name)
        .collect()
}
fn dom_value(element: ElementRef<'_>) -> String {
    ["content", "resource", "href", "src", "datetime"]
        .iter()
        .find_map(|attr| element.value().attr(attr))
        .map(str::to_owned)
        .unwrap_or_else(|| element.text().collect::<String>().trim().to_string())
}
fn dom_node(element: ElementRef<'_>) -> Value {
    let mut object = serde_json::Map::new();
    let kinds = type_tokens(element);
    if !kinds.is_empty() {
        object.insert("@type".into(), json!(kinds));
    }
    if let Some(id) = element
        .value()
        .attr("itemid")
        .or_else(|| element.value().attr("about"))
    {
        object.insert("@id".into(), json!(id));
    }
    for child in element
        .descendants()
        .filter_map(ElementRef::wrap)
        .filter(|child| child.id() != element.id())
    {
        let props = property_tokens(child);
        if props.is_empty() {
            continue;
        }
        let nearest = child
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find(|ancestor| !type_tokens(*ancestor).is_empty());
        if nearest.is_none_or(|parent| parent.id() != element.id()) {
            continue;
        }
        let value = if type_tokens(child).is_empty() {
            json!(dom_value(child))
        } else {
            dom_node(child)
        };
        for property in props {
            match object.get_mut(property) {
                None => {
                    object.insert(property.into(), value.clone());
                }
                Some(Value::Array(values)) => values.push(value.clone()),
                Some(previous) => {
                    *previous = json!([previous.clone(), value.clone()]);
                }
            }
        }
    }
    Value::Object(object)
}
fn structured_dom_products(document: &Html) -> Vec<Value> {
    let scopes = Selector::parse("[itemtype], [typeof]").expect("static selector");
    document
        .select(&scopes)
        .filter(|element| {
            !element
                .ancestors()
                .filter_map(ElementRef::wrap)
                .any(|ancestor| !type_tokens(ancestor).is_empty())
        })
        .map(dom_node)
        .collect()
}
fn open_graph_product(document: &Html) -> Option<ProductMetadata> {
    let selector = Selector::parse("meta[property], meta[name]").ok()?;
    let mut meta = HashMap::new();
    for element in document.select(&selector) {
        if let Some(key) = element
            .value()
            .attr("property")
            .or_else(|| element.value().attr("name"))
        {
            meta.entry(key.to_ascii_lowercase())
                .or_insert_with(|| dom_value(element));
        }
    }
    if !meta
        .get("og:type")
        .is_some_and(|kind| matches!(kind.as_str(), "product" | "product.item"))
    {
        return None;
    }
    let pair = [
        ("product:price:amount", "product:price:currency"),
        ("og:price:amount", "og:price:currency"),
    ]
    .iter()
    .find_map(|(p, c)| {
        meta.get(*p)
            .and_then(|s| decimal_price(s))
            .zip(meta.get(*c).and_then(|s| currency_code(s)))
    });
    let (price, currency) = pair
        .map(|(p, c)| (Some(p), Some(c)))
        .unwrap_or((None, None));
    Some(ProductMetadata {
        version: 1,
        price,
        currency,
        price_is_from: false,
        captured_at: Some(crate::time::now_utc_iso()),
    })
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_broadcast()
                && !ip.is_documentation()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && o[0] != 0
                && o[0] < 224
                && !(o[0] == 100 && (64..=127).contains(&o[1]))
                && !(o[0] == 198 && (o[1] == 18 || o[1] == 19))
                && !(o[0] == 192 && o[1] == 0 && o[2] == 0)
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let s = ip.segments();
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && (s[0] & 0xfe00) != 0xfc00
                && (s[0] & 0xffc0) != 0xfe80
                && !(s[0] == 0x2001 && s[1] == 0xdb8)
                && (s[0] & 0xe000) == 0x2000
        }
    }
}
fn approved_addresses(url: &Url) -> Result<Vec<std::net::SocketAddr>> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.port_or_known_default(), Some(80 | 443))
    {
        bail!("product metadata requires a public HTTP(S) URL");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("missing product host"))?;
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        bail!("non-public product host");
    }
    let addresses: Vec<_> = (host, url.port_or_known_default().unwrap())
        .to_socket_addrs()?
        .collect();
    if addresses.is_empty() || addresses.iter().any(|a| !public_ip(a.ip())) {
        bail!("non-public product address");
    }
    Ok(addresses)
}

/// Optional bounded retrieval for URL-only native ingress. Every redirect is
/// checked again and DNS is pinned to approved public addresses. Failures must
/// remain best effort at callers so offline link saves continue to work.
pub fn fetch_product_metadata(url: &str) -> Result<Option<ProductMetadata>> {
    let deadline = Instant::now() + FETCH_DEADLINE;
    let mut current = Url::parse(url)?;
    for hop in 0..=3 {
        let addresses = approved_addresses(&current)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("product metadata deadline exceeded");
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(remaining.min(Duration::from_secs(3)))
            .timeout(remaining)
            .resolve_to_addrs(current.host_str().unwrap(), &addresses)
            .build()?;
        let mut response = client
            .get(current.clone())
            .header(ACCEPT, "text/html,application/xhtml+xml")
            .header("User-Agent", "Mozilla/5.0 Óia/0.1 ProductMetadata")
            .send()?;
        if response.status().is_redirection() {
            if hop == 3 {
                bail!("too many product metadata redirects");
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| anyhow!("missing product redirect"))?;
            current = current.join(location)?;
            continue;
        }
        if !response.status().is_success() {
            bail!("product metadata HTTP {}", response.status());
        }
        if !response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/html") || v.starts_with("application/xhtml+xml"))
        {
            return Ok(None);
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_PRODUCT_HTML_BYTES as u64)
        {
            bail!("product HTML exceeded size limit");
        }
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take((MAX_PRODUCT_HTML_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_PRODUCT_HTML_BYTES {
            bail!("product HTML exceeded size limit");
        }
        let html = String::from_utf8_lossy(&bytes);
        return Ok(parse_product_metadata(&html, current.as_str()));
    }
    Ok(None)
}

/// Add a product snapshot to an unchanged reading without replacing content,
/// assets, save date, tags, or user state. The complete article fingerprint is
/// checked while locked and again immediately before the atomic write.
pub fn update_product_metadata_if_unchanged(
    library: &LibraryRoot,
    id: &str,
    product: ProductMetadata,
    expected_article_sha256: &str,
) -> Result<bool> {
    if id.is_empty()
        || std::path::Path::new(id).components().count() != 1
        || !matches!(
            std::path::Path::new(id).components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        bail!("invalid reading id");
    }
    if expected_article_sha256.len() != 64
        || !expected_article_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("invalid article snapshot");
    }
    let product =
        validate_product(product).ok_or_else(|| anyhow!("invalid product metadata version"))?;
    let lock = crate::locking::lock_reading(library, id)?;
    let path = library.article_path(id);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    if crate::sha256_hex(&bytes) != expected_article_sha256 {
        return Ok(false);
    }
    let reading = crate::parse_reading(std::str::from_utf8(&bytes)?)?;
    if reading.metadata.id != id
        || reading.metadata.kind != ReadingKind::Article
        || !reading.metadata.lightweight
        || reading.metadata.source_profile.is_some()
    {
        return Ok(false);
    }
    lock.ensure_protects(library, id)?;
    let original = std::str::from_utf8(&bytes)?;
    let rest = original
        .strip_prefix("---\n")
        .ok_or_else(|| anyhow!("missing frontmatter"))?;
    let close = rest
        .find("\n---\n")
        .ok_or_else(|| anyhow!("missing frontmatter close"))?;
    let header = &rest[..close];
    // Replace only the top-level product block. Preserve unknown YAML keys,
    // whitespace, body bytes, and source_hash exactly as the external writer left them.
    let mut retained = String::new();
    let mut skipping = false;
    for line in header.split_inclusive('\n') {
        let top_level = !line.starts_with([' ', '\t']) && !line.trim().is_empty();
        if top_level && !line.starts_with('#') {
            skipping = line
                .split_once(':')
                .is_some_and(|(key, _)| key.trim().trim_matches(['\'', '"']) == "product");
        }
        if !skipping || (top_level && line.starts_with('#')) {
            retained.push_str(line);
        }
    }
    let yaml = serde_yaml::to_string(&product)?;
    let block = yaml
        .lines()
        .map(|line| format!("  {line}\n"))
        .collect::<String>();
    let result = format!(
        "---\n{}{}product:\n{}{}",
        retained,
        if retained.ends_with('\n') { "" } else { "\n" },
        block,
        &rest[close + 1..]
    );
    let mut staged = tempfile::NamedTempFile::new_in(library.reading_dir(id))?;
    use std::io::Write;
    staged
        .as_file()
        .set_permissions(fs::metadata(&path)?.permissions())?;
    staged.write_all(result.as_bytes())?;
    staged.as_file().sync_all()?;
    if fs::read(&path).is_ok_and(|bytes| crate::sha256_hex(&bytes) == expected_article_sha256) {
        staged.persist(&path).map_err(|error| error.error)?;
        fs::File::open(library.reading_dir(id))?.sync_all()?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(data: &str) -> Option<ProductMetadata> {
        parse_product_metadata(
            &format!("<script type='application/ld+json'>{data}</script>"),
            "https://shop.example/lamp",
        )
    }
    #[test]
    fn single_product_and_currency_snapshot() {
        let p=parse(r#"{"@type":"Product","offers":{"@type":"Offer","price":"00125.50","priceCurrency":"eur"}}"#).unwrap();
        assert_eq!(p.price.as_deref(), Some("125.5"));
        assert_eq!(p.currency.as_deref(), Some("EUR"));
        assert!(!p.price_is_from);
    }
    #[test]
    fn graph_main_entity_and_offer_id() {
        let p=parse(r##"{"@graph":[{"@type":"WebPage","mainEntity":{"@id":"#lamp"}},{"@id":"#lamp","@type":["Thing","Product"],"offers":{"@id":"#offer"}},{"@id":"#offer","@type":"Offer","price":95,"priceCurrency":"GBP"}]}"##).unwrap();
        assert_eq!(p.price.as_deref(), Some("95"));
    }
    #[test]
    fn recommended_product_list_and_review_are_not_products() {
        for data in [
            r#"{"@type":"ItemList","itemListElement":[{"@type":"Product","offers":{"price":2,"priceCurrency":"EUR"}}]}"#,
            r#"{"@type":"Review","itemReviewed":{"@type":"Product"}}"#,
            r#"[{"@type":"Article"},{"@type":"Product"}]"#,
        ] {
            assert!(parse(data).is_none(), "{data}");
        }
    }
    #[test]
    fn select_main_url_over_related_graph_product() {
        let p=parse(r#"[{"@type":"Product","url":"https://shop.example/related","offers":{"price":1,"priceCurrency":"EUR"}},{"@type":"Product","url":"https://shop.example/lamp","offers":{"price":20,"priceCurrency":"EUR"}}]"#).unwrap();
        assert_eq!(p.price.as_deref(), Some("20"));
    }
    #[test]
    fn variant_offer_url_and_ranges() {
        let data = r#"{"@type":"ProductGroup","url":"https://shop.example/lamp","hasVariant":[{"@type":"Product","url":"https://shop.example/lamp?variant=2","offers":{"price":"44.50","priceCurrency":"EUR"}}],"offers":{"@type":"AggregateOffer","lowPrice":30,"highPrice":80,"priceCurrency":"EUR"}}"#;
        let html = format!("<script type='application/ld+json'>{data}</script>");
        let variant = parse_product_metadata(&html, "https://shop.example/lamp?variant=2").unwrap();
        assert_eq!(variant.price.as_deref(), Some("44.5"));
        assert!(!variant.price_is_from);
        let p = parse(data).unwrap();
        assert_eq!(p.price.as_deref(), Some("30"));
        assert!(p.price_is_from);
    }
    #[test]
    fn invalid_price_does_not_hide_product() {
        for offer in [
            r#"{"price":"1,000","priceCurrency":"EUR"}"#,
            r#"{"price":"22","priceCurrency":"???"}"#,
            r#"{"price":"-2","priceCurrency":"USD"}"#,
            r#"{"price":44}"#,
        ] {
            let p = parse(&format!(r#"{{"@type":"Product","offers":{offer}}}"#)).unwrap();
            assert!(p.price.is_none());
            assert!(p.currency.is_none());
        }
    }
    #[test]
    fn scoped_microdata_and_rdfa() {
        for html in [
            r#"<div itemscope itemtype="https://schema.org/Product"><div itemprop="offers" itemscope itemtype="https://schema.org/Offer"><meta itemprop="price" content="42.99"><meta itemprop="priceCurrency" content="USD"></div></div>"#,
            r#"<div vocab="https://schema.org/" typeof="Product"><div property="offers" typeof="Offer"><meta property="price" content="42.99"><meta property="priceCurrency" content="USD"></div></div>"#,
        ] {
            assert_eq!(
                parse_product_metadata(html, "https://shop.example/lamp")
                    .unwrap()
                    .price
                    .as_deref(),
                Some("42.99")
            );
        }
    }
    #[test]
    fn og_requires_product_evidence_and_paired_currency() {
        let tags = r#"<meta property="product:price:amount" content="250"><meta property="product:price:currency" content="EUR">"#;
        assert!(parse_product_metadata(tags, "https://shop.example/lamp").is_none());
        assert_eq!(
            parse_product_metadata(
                &format!("<meta property='og:type' content='product'>{tags}"),
                "https://shop.example/lamp"
            )
            .unwrap()
            .price
            .as_deref(),
            Some("250")
        );
    }
    #[test]
    fn no_text_or_url_heuristics() {
        assert!(parse_product_metadata(
            "<h1>Product lamp €200</h1>",
            "https://shop.example/products/lamp"
        )
        .is_none());
    }
    #[test]
    fn rejects_private_network_fetch() {
        for url in [
            "http://127.0.0.1/product",
            "http://192.168.1.2/product",
            "http://[::1]/product",
            "http://user:password@example.com/product",
        ] {
            assert!(fetch_product_metadata(url).is_err());
        }
    }
    #[test]
    fn referenced_review_or_collection_product_stays_nonproduct() {
        for context in ["Review", "CollectionPage", "ItemList"] {
            let data = format!(
                r##"{{"@graph":[{{"@type":"{context}","itemReviewed":{{"@id":"#lamp"}}}},{{"@type":"Product","@id":"#lamp","url":"https://shop.example/lamp","offers":{{"price":25,"priceCurrency":"EUR"}}}}]}}"##
            );
            assert!(parse(&data).is_none(), "{context}");
        }
    }
    #[test]
    fn nested_product_reviews_do_not_hide_main_product() {
        assert!(parse(r#"{"@type":"Product","review":{"@type":"Review"},"offers":{"price":25,"priceCurrency":"EUR"}}"#).is_some());
    }
    #[test]
    fn unrelated_webpage_cannot_designate_main_product() {
        assert!(parse(r#"{"@type":"WebPage","url":"https://other.example/page","mainEntity":{"@type":"Product"}}"#).is_none());
    }
    #[test]
    fn article_context_carries_to_dom_fallback() {
        let html = r#"<script type="application/ld+json">{"@type":"Article"}</script><div itemscope itemtype="https://schema.org/Product"><meta itemprop="name" content="Related lamp"></div>"#;
        assert!(parse_product_metadata(html, "https://shop.example/lamp").is_none());
    }
    #[test]
    fn selected_variant_fragment_ids_do_not_override_url() {
        let data = r##"{"@type":"ProductGroup","hasVariant":[{"@type":"Product","@id":"#red","url":"/lamp?variant=1","offers":{"@id":"#red-offer","url":"/lamp?variant=1","price":10,"priceCurrency":"EUR"}},{"@type":"Product","@id":"#blue","url":"/lamp?variant=2","offers":{"@id":"#blue-offer","url":"/lamp?variant=2","price":20,"priceCurrency":"EUR"}}]}"##;
        let html = format!("<script type='application/ld+json'>{data}</script>");
        let p = parse_product_metadata(&html, "https://shop.example/lamp?variant=2").unwrap();
        assert_eq!(p.price.as_deref(), Some("20"));
    }
    #[test]
    fn malformed_expiry_and_mime_parameters_do_not_panic() {
        let html = r#"<script type="APPLICATION/LD+JSON; charset=utf-8">{"@type":"Product","offers":{"price":25,"priceCurrency":"EUR","priceValidUntil":"123456789💡"}}</script>"#;
        assert_eq!(
            parse_product_metadata(html, "https://shop.example/lamp")
                .unwrap()
                .price
                .as_deref(),
            Some("25")
        );
    }
    #[test]
    fn migration_preserves_raw_body_unknown_fields_and_snapshot_guard() {
        let dir = tempfile::TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = crate::import_link(&library, "https://shop.example/lamp").unwrap();
        let path = library.article_path(&saved.id);
        let initial = fs::read_to_string(&path).unwrap();
        let close = initial.find("\n---\n").unwrap();
        let original=format!("{}\nunknown_future_key: 'Keep exactly this!'\n# preserved comment{}\nBody with trailing spaces  \n\n\n",&initial[..close],&initial[close..close+6]);
        fs::write(&path, &original).unwrap();
        let expected = crate::sha256_hex(original.as_bytes());
        let product =
            parse(r#"{"@type":"Product","offers":{"price":"140.00","priceCurrency":"EUR"}}"#)
                .unwrap();
        assert!(update_product_metadata_if_unchanged(
            &library,
            &saved.id,
            product.clone(),
            &expected
        )
        .unwrap());
        let changed = fs::read_to_string(&path).unwrap();
        assert!(changed.contains("unknown_future_key: 'Keep exactly this!'\n# preserved comment"));
        assert_eq!(
            original.split_once("\n---\n").unwrap().1,
            changed.split_once("\n---\n").unwrap().1
        );
        let previous = crate::parse_reading(&original).unwrap();
        let parsed = crate::parse_reading(&changed).unwrap();
        let mut metadata = previous.metadata;
        metadata.product = Some(product.clone());
        assert_eq!(metadata, parsed.metadata);
        assert!(
            !update_product_metadata_if_unchanged(&library, &saved.id, product, &expected).unwrap()
        );
        assert_eq!(fs::read_to_string(path).unwrap(), changed);
    }
    #[test]
    fn product_roundtrip_index_ffi_and_full_capture_stays_link() {
        let dir = tempfile::TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let product =
            parse(r#"{"@type":"Product","offers":{"price":140,"priceCurrency":"EUR"}}"#).unwrap();
        let input = crate::SaveLinkInput {
            url: "https://shop.example/lamp".into(),
            canonical_url: "https://shop.example/lamp".into(),
            title: "Lamp".into(),
            author: None,
            site: None,
            saved_at: "2026-10-10T12:00:00.000Z".into(),
            images: vec![],
            preview_url: None,
            favicon_url: None,
            theme_color: None,
            product: Some(product.clone()),
            excerpt: None,
            lang: None,
        };
        let saved = crate::save_link_capture(&library, input).unwrap();
        let reading =
            crate::parse_reading(&fs::read_to_string(library.article_path(&saved.id)).unwrap())
                .unwrap();
        assert_eq!(reading.metadata.product.as_ref(), Some(&product));
        let upgraded = crate::save_capture(
            &library,
            crate::SaveInput {
                quote_identity_markdown: None,
                kind: ReadingKind::Article,
                lightweight: false,
                url: "https://shop.example/lamp".into(),
                media_url: None,
                canonical_url: "https://shop.example/lamp".into(),
                title: "Lamp".into(),
                author: None,
                site: None,
                saved_at: "2026-10-10T13:00:00.000Z".into(),
                markdown: "Full captured product information".into(),
                images: vec![],
                preview_url: None,
                favicon_url: None,
                theme_color: None,
                product: None,
                excerpt: None,
                word_count: None,
                lang: None,
            },
        )
        .unwrap();
        assert_eq!(upgraded.disposition, crate::SaveDisposition::Upgraded);
        let conn = crate::open_index(&dir.path().join("index.db")).unwrap();
        crate::rebuild(&conn, &library).unwrap();
        let rows = crate::list_readings(
            &conn,
            &crate::ListOptions {
                view: crate::View::Links,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_link);
        assert!(!rows[0].lightweight);
        let indexed: ProductMetadata =
            serde_json::from_str(rows[0].product_json.as_deref().unwrap()).unwrap();
        assert_eq!(indexed, product);
        let ffi = crate::ffi::FfiReadingRow::from(rows[0].clone());
        assert_eq!(ffi.product_json, rows[0].product_json);
        assert!(crate::list_readings(
            &conn,
            &crate::ListOptions {
                view: crate::View::Articles,
                ..Default::default()
            }
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn relative_tala_product_offer_is_exact_price() {
        let p=parse(r#"{"@type":"Product","url":"/lamp","offers":[{"@type":"Offer","price":140.0,"priceCurrency":"EUR","url":"/lamp?variant=46805536702798"}]}"#).unwrap();
        assert_eq!(p.price.as_deref(), Some("140"));
        assert_eq!(p.currency.as_deref(), Some("EUR"));
        assert!(!p.price_is_from);
    }
    #[test]
    fn product_group_without_parent_offers_has_from_price() {
        let p=parse(r#"{"@type":"ProductGroup","url":"/lamp","hasVariant":[{"@type":"Product","url":"/lamp?variant=1","offers":{"price":20,"priceCurrency":"EUR"}},{"@type":"Product","url":"/lamp?variant=2","offers":{"price":40,"priceCurrency":"EUR"}}]}"#).unwrap();
        assert_eq!(p.price.as_deref(), Some("20"));
        assert!(p.price_is_from);
    }
    #[test]
    fn singleton_product_at_another_url_is_not_main_product() {
        assert!(parse(r#"{"@type":"Product","url":"https://shop.example/other","offers":{"price":10,"priceCurrency":"EUR"}}"#).is_none());
    }
    #[test]
    fn main_product_price_falls_back_to_explicit_og_but_keeps_current_offer() {
        let og = r#"<meta property="og:type" content="product"><meta property="product:price:amount" content="140"><meta property="product:price:currency" content="EUR">"#;
        let fallback =
            format!(r#"<script type="application/ld+json">{{"@type":"Product"}}</script>{og}"#);
        assert_eq!(
            parse_product_metadata(&fallback, "https://shop.example/lamp")
                .unwrap()
                .price
                .as_deref(),
            Some("140")
        );
        let preferred = format!(
            r#"<script type="application/ld+json">{{"@type":"Product","offers":{{"price":125,"priceCurrency":"EUR"}}}}</script>{og}"#
        );
        assert_eq!(
            parse_product_metadata(&preferred, "https://shop.example/lamp")
                .unwrap()
                .price
                .as_deref(),
            Some("125")
        );
        let dom = format!(
            r#"<div itemscope itemtype="https://schema.org/Product"><meta itemprop="name" content="Lamp"></div>{og}"#
        );
        assert_eq!(
            parse_product_metadata(&dom, "https://shop.example/lamp")
                .unwrap()
                .price
                .as_deref(),
            Some("140")
        );
    }
}
