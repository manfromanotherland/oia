// SPDX-License-Identifier: MIT

//! Bounded HTML content transported by the iOS Shortcut. The Shortcut supplies
//! observations; extraction, Markdown, and article classification stay in Rust.

use anyhow::{ensure, Result};
use base64::Engine as _;
use scraper::{ElementRef, Html, Node, Selector};
use url::Url;

use crate::{ImageBytes, PageCaptureEvidence, MAX_PAGE_CAPTURE_PARAGRAPHS};

pub(crate) const MAX_ARTICLE_HTML_BYTES: usize = 8 * 1024 * 1024;
const MAX_RENDER_DEPTH: usize = 64;
const MAX_INLINE_IMAGE_BYTES: usize = 40 * 1024 * 1024;

pub(crate) struct ArticleHtml {
    pub page_url: String,
    pub markdown: String,
    pub structured_html: String,
    pub evidence: PageCaptureEvidence,
    pub title: Option<String>,
    pub canonical_url: Option<String>,
    pub site: Option<String>,
    pub images: Vec<ImageBytes>,
}

fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("static article selector")
}

fn excluded(element: ElementRef<'_>) -> bool {
    let page_chrome = matches!(element.value().name(), "header" | "footer")
        && !element
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|ancestor| {
                matches!(ancestor.value().name(), "article" | "main")
                    || ancestor.attr("role") == Some("main")
            });
    element.attr("hidden").is_some()
        || element.attr("aria-hidden") == Some("true")
        || page_chrome
        || matches!(
            element.value().name(),
            "script"
                | "style"
                | "noscript"
                | "template"
                | "nav"
                | "aside"
                | "form"
                | "button"
                | "input"
                | "select"
                | "svg"
                | "canvas"
                | "iframe"
        )
}

fn visible(element: ElementRef<'_>) -> bool {
    !excluded(element)
        && !element
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(excluded)
}

fn visible_text(element: ElementRef<'_>, depth: usize) -> Result<String> {
    ensure!(
        depth <= MAX_RENDER_DEPTH,
        "The article HTML is nested too deeply."
    );
    if excluded(element) {
        return Ok(String::new());
    }
    let mut output = String::new();
    for node in element.children() {
        match node.value() {
            Node::Text(text) => output.push_str(text),
            Node::Element(_) => {
                output.push_str(&visible_text(ElementRef::wrap(node).unwrap(), depth + 1)?)
            }
            _ => {}
        }
    }
    Ok(output)
}

fn normalized_text(text: &str) -> String {
    let mut output = String::new();
    let mut space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if !space {
                output.push(' ');
            }
            space = true;
        } else {
            output.push(character);
            space = false;
        }
    }
    output
}

fn escaped(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('`', "\\`")
}

fn destination(base: &Url, raw: &str, image: bool) -> Option<String> {
    let url = base.join(raw.trim()).ok()?;
    let permitted =
        matches!(url.scheme(), "http" | "https") || (!image && url.scheme() == "mailto");
    (permitted && url.username().is_empty() && url.password().is_none())
        .then(|| url.to_string().replace('>', "%3E").replace('<', "%3C"))
}

struct Renderer<'a> {
    base: &'a Url,
    images: Vec<ImageBytes>,
    inline_bytes: usize,
}

impl Renderer<'_> {
    fn children(&mut self, element: ElementRef<'_>, depth: usize) -> Result<String> {
        let mut output = String::new();
        for node in element.children() {
            match node.value() {
                Node::Text(text) => output.push_str(&escaped(&normalized_text(text))),
                Node::Element(_) => {
                    output.push_str(&self.element(ElementRef::wrap(node).unwrap(), depth + 1)?)
                }
                _ => {}
            }
        }
        Ok(output)
    }

    fn element(&mut self, element: ElementRef<'_>, depth: usize) -> Result<String> {
        ensure!(
            depth <= MAX_RENDER_DEPTH,
            "The article HTML is nested too deeply."
        );
        if excluded(element) {
            return Ok(String::new());
        }
        let name = element.value().name();
        if name == "img" {
            let alt = escaped(element.attr("alt").unwrap_or("Article image"));
            let Some(raw) = element.attr("src").or_else(|| element.attr("data-src")) else {
                return Ok(alt);
            };
            let target = if let Some(data) = raw.strip_prefix("data:") {
                let image = data.split_once(',').and_then(|(header, bytes)| {
                    let content_type = header.strip_suffix(";base64")?;
                    if !content_type.starts_with("image/") {
                        return None;
                    }
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(bytes)
                        .ok()?;
                    if self.inline_bytes.checked_add(bytes.len())? > MAX_INLINE_IMAGE_BYTES
                        || crate::inbox::image_type(&bytes).is_none()
                    {
                        return None;
                    }
                    Some(ImageBytes {
                        url: raw.to_string(),
                        content_type: content_type.to_string(),
                        bytes,
                    })
                });
                let Some(image) = image else {
                    return Ok(alt);
                };
                self.inline_bytes += image.bytes.len();
                self.images.push(image);
                Some(raw.to_string())
            } else {
                destination(self.base, raw, true)
            };
            return Ok(target
                .map(|url| format!("![{alt}](<{url}>)"))
                .unwrap_or(alt));
        }
        if name == "pre" {
            let code = visible_text(element, depth)?;
            let fence = if code.contains("```") { "````" } else { "```" };
            return Ok(format!("\n\n{fence}\n{}\n{fence}\n\n", code.trim_end()));
        }
        let inner = self.children(element, depth)?;
        Ok(match name {
            "p" | "div" | "section" | "article" | "main" | "figure" | "figcaption" => {
                format!("\n\n{}\n\n", inner.trim())
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => format!(
                "\n\n{} {}\n\n",
                "#".repeat(name.as_bytes()[1].saturating_sub(b'0') as usize),
                inner.trim()
            ),
            "strong" | "b" => format!("**{inner}**"),
            "em" | "i" => format!("*{inner}*"),
            "code" => format!("`{}`", visible_text(element, depth)?.replace('`', "\\`")),
            "br" => "\n".into(),
            "hr" => "\n\n---\n\n".into(),
            "a" => element
                .attr("href")
                .and_then(|raw| destination(self.base, raw, false))
                .map(|url| format!("[{}](<{url}>)", inner.trim()))
                .unwrap_or(inner),
            "blockquote" => format!(
                "\n\n{}\n\n",
                inner
                    .trim()
                    .lines()
                    .map(|line| format!("> {line}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            "li" => format!("\n- {}", inner.trim().replace('\n', "\n  ")),
            "ul" | "ol" => format!("\n\n{}\n\n", inner.trim()),
            "tr" => format!("\n{}\n", inner.trim()),
            "th" | "td" => format!(" {} |", inner.trim()),
            _ => inner,
        })
    }
}

/// Keep metadata bounded even when the actual article is several MiB. Passing
/// the whole document to the metadata classifier would turn large prose into a
/// link merely because it exceeded that classifier's separate evidence limit.
fn structured_html(document: &Html, raw: &str) -> String {
    let limit = crate::product::MAX_BROWSER_PRODUCT_HTML_BYTES;
    if raw.len() <= limit {
        return raw.to_string();
    }
    let mut output = String::new();
    for element in document.select(&selector("meta, script[type='application/ld+json']")) {
        let html = element.html();
        if output.len() + html.len() <= limit {
            output.push_str(&html);
        }
    }
    output
}

pub(crate) fn parse_article_html(html: &str, page_url: &str) -> Result<ArticleHtml> {
    ensure!(
        html.len() <= MAX_ARTICLE_HTML_BYTES,
        "The article HTML exceeds 8 MiB."
    );
    let base = Url::parse(page_url)?;
    let document = Html::parse_document(html);
    let root = document
        .select(&selector("article"))
        .filter(|element| visible(*element))
        .max_by_key(|element| element.text().map(str::len).sum::<usize>())
        .or_else(|| {
            document
                .select(&selector("main, [role='main']"))
                .find(|element| visible(*element))
        })
        .or_else(|| document.select(&selector("body")).next())
        .unwrap_or_else(|| document.root_element());
    let text = normalized_text(&visible_text(root, 0)?);
    let count = |value: &str| value.trim().chars().count().min(u32::MAX as usize) as u32;
    let mut evidence = PageCaptureEvidence {
        text_characters: count(&text),
        ..PageCaptureEvidence::default()
    };
    for element in root
        .select(&selector("a"))
        .filter(|element| visible(*element))
    {
        evidence.link_characters = evidence
            .link_characters
            .saturating_add(count(&visible_text(element, 0)?));
    }
    evidence.link_characters = evidence.link_characters.min(evidence.text_characters);
    for element in root
        .select(&selector("p"))
        .filter(|element| visible(*element))
    {
        if evidence.paragraph_characters.len() == MAX_PAGE_CAPTURE_PARAGRAPHS {
            break;
        }
        evidence
            .paragraph_characters
            .push(count(&visible_text(element, 0)?).min(evidence.text_characters));
    }
    let mut renderer = Renderer {
        base: &base,
        images: vec![],
        inline_bytes: 0,
    };
    let rendered = renderer.element(root, 0)?;
    let mut lines = Vec::new();
    let mut blank = false;
    for line in rendered.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            if !blank {
                lines.push("");
            }
            blank = true;
        } else {
            lines.push(line);
            blank = false;
        }
    }
    let meta = |property: &str| {
        document
            .select(&selector("meta"))
            .find(|element| {
                element.attr("property").or_else(|| element.attr("name")) == Some(property)
            })
            .and_then(|element| element.attr("content"))
            .map(str::to_string)
    };
    let title = meta("og:title")
        .or_else(|| {
            document
                .select(&selector("title"))
                .next()
                .map(|element| element.text().collect::<String>())
        })
        .or_else(|| {
            root.select(&selector("h1"))
                .find(|element| visible(*element))
                .and_then(|element| visible_text(element, 0).ok())
        })
        .filter(|text| !text.trim().is_empty());
    let canonical_url = document
        .select(&selector("link[rel='canonical']"))
        .next()
        .and_then(|element| element.attr("href"))
        .and_then(|raw| destination(&base, raw, true));
    Ok(ArticleHtml {
        page_url: page_url.to_string(),
        markdown: lines.join("\n").trim().to_string(),
        structured_html: structured_html(&document, html),
        evidence,
        title,
        canonical_url,
        site: meta("og:site_name"),
        images: renderer.images,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_article_omits_chrome_and_preserves_formatting_and_relative_origins() {
        let html = "<title>Page title</title><nav>Navigation</nav><article><h1>Heading</h1><p>A <strong>bold</strong> thought with <a href='/more'>a link</a>.</p><img src='/image.png' alt='Local artwork'><blockquote><p>Quoted words.</p></blockquote><pre>let x = 1;\nlet y = 2;</pre></article><footer>Footer</footer>";
        let article = parse_article_html(html, "https://example.com/post").unwrap();
        assert_eq!(article.title.as_deref(), Some("Page title"));
        assert!(!article.markdown.contains("##"));
        assert!(article.markdown.contains("# Heading"));
        assert!(article.markdown.contains("**bold**"));
        assert!(article
            .markdown
            .contains("[a link](<https://example.com/more>)"));
        assert!(article
            .markdown
            .contains("![Local artwork](<https://example.com/image.png>)"));
        assert!(article.markdown.contains("> Quoted words."));
        assert!(article.markdown.contains("let x = 1;\nlet y = 2;"));
        assert!(!article.markdown.contains("Navigation"));
        assert!(!article.markdown.contains("Footer"));
        assert!(article.evidence.is_valid());
    }

    #[test]
    fn metadata_stays_bounded_independently_of_large_article_body() {
        let html = format!(
            "<meta property='og:type' content='article'><article><p>{}</p></article>",
            "Useful prose. ".repeat(50_000)
        );
        let article = parse_article_html(&html, "https://example.com/post").unwrap();
        assert!(article.structured_html.len() <= crate::product::MAX_BROWSER_PRODUCT_HTML_BYTES);
        assert_eq!(
            crate::classify_page_capture(
                "https://example.com/post",
                &article.structured_html,
                &article.markdown,
                Some(&article.evidence)
            ),
            crate::PageCaptureKind::Article
        );
    }

    #[test]
    fn dangerous_destinations_and_hidden_text_are_omitted() {
        let article = parse_article_html("<main><p>Keep <span hidden>secret</span>this.</p><a href='javascript:alert(1)'>safe label</a><img src='file:///Users/ed/private.png'></main>", "https://example.com/post").unwrap();
        assert!(!article.markdown.contains("secret"));
        assert!(!article.markdown.contains("javascript:"));
        assert!(!article.markdown.contains("file:"));
    }

    #[test]
    fn semantic_headers_keep_the_title_and_lead_paragraph() {
        let article = parse_article_html("<article><header><h1>Story title</h1><p>The important lead.</p></header><p>The remaining story.</p></article>", "https://example.com/post").unwrap();
        assert_eq!(article.title.as_deref(), Some("Story title"));
        assert!(article.markdown.contains("The important lead."));
        assert_eq!(article.evidence.paragraph_characters.len(), 2);
    }

    #[test]
    fn excluded_ancestors_never_supply_prose_or_link_evidence() {
        let html = format!(
            "<body><nav><a href='/home'>{}</a></nav><p>{}</p><p>{}</p></body>",
            "Navigation link. ".repeat(100),
            "Visible article prose. ".repeat(20),
            "Another useful paragraph. ".repeat(20)
        );
        let article = parse_article_html(&html, "https://example.com/post").unwrap();
        assert_eq!(article.evidence.link_characters, 0);
        assert_eq!(
            crate::classify_page_capture(
                "https://example.com/post",
                &article.structured_html,
                &article.markdown,
                Some(&article.evidence)
            ),
            crate::PageCaptureKind::Article
        );
        let hidden = format!(
            "<div hidden><p>{}</p><article><p>{}</p></article></div><p>Visible labels.</p>",
            "Hidden prose. ".repeat(100),
            "Hidden article. ".repeat(100)
        );
        let article = parse_article_html(&hidden, "https://example.com/post").unwrap();
        assert!(!article.markdown.contains("Hidden"));
        assert_eq!(article.evidence.paragraph_characters, vec![15]);
        assert_eq!(
            crate::classify_page_capture(
                "https://example.com/post",
                &article.structured_html,
                &article.markdown,
                Some(&article.evidence)
            ),
            crate::PageCaptureKind::Link
        );
    }
}
