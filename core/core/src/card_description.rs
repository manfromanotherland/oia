// SPDX-License-Identifier: MIT

use crate::ReadingKind;

/// A disposable board projection. The saved excerpt and word count stay in
/// frontmatter exactly as the writer received them.
pub(crate) struct ArticleCardProjection {
    pub description: Option<String>,
    pub word_count: Option<u32>,
}

const MAX_DESCRIPTION_CHARS: usize = 420;

pub(crate) fn project_article_card(
    kind: ReadingKind,
    lightweight: bool,
    source_type: Option<&str>,
    title: &str,
    excerpt: Option<&str>,
    saved_word_count: Option<u32>,
    body: &str,
) -> ArticleCardProjection {
    if kind != ReadingKind::Article || lightweight || source_type == Some("social_post") {
        return ArticleCardProjection {
            description: None,
            word_count: saved_word_count,
        };
    }

    let saved_description = excerpt
        .map(str::trim)
        .filter(|value| !value.is_empty() && !same_heading(value, title));
    let description = saved_description
        .map(|value| limit_description(value.to_string()))
        .or_else(|| description_from_body(body, title));
    let word_count = saved_word_count.filter(|count| *count > 0).or_else(|| {
        let words = body
            .split_whitespace()
            .filter(|word| word.chars().any(char::is_alphanumeric))
            .count();
        (words > 0).then_some(words.min(u32::MAX as usize) as u32)
    });

    ArticleCardProjection {
        description,
        word_count,
    }
}

fn same_heading(a: &str, b: &str) -> bool {
    let normalized = |value: &str| {
        value
            .chars()
            .filter(|character| character.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    normalized(a) == normalized(b)
}

fn description_from_body(body: &str, title: &str) -> Option<String> {
    let mut description = String::new();
    let mut fenced = false;

    for line in body.lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.is_empty() || is_rule(line) || line.starts_with("<!--") {
            continue;
        }
        let line = strip_block_markup(line);
        let cleaned = clean_inline_markup(line);
        let cleaned = cleaned.trim();
        if cleaned.is_empty() || same_heading(cleaned, title) {
            continue;
        }
        if !description.is_empty() {
            description.push(' ');
        }
        description.push_str(cleaned);
        if description.chars().count() >= MAX_DESCRIPTION_CHARS {
            break;
        }
    }

    (!description.is_empty()).then(|| limit_description(description))
}

fn is_rule(line: &str) -> bool {
    let compact = line
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    compact.len() >= 3
        && compact
            .chars()
            .all(|character| character == '-' || character == '*' || character == '_')
}

fn strip_block_markup(mut line: &str) -> &str {
    loop {
        let trimmed = line.trim_start();
        let next = if let Some(rest) = trimmed.strip_prefix('>') {
            Some(rest)
        } else if let Some(rest) = trimmed.strip_prefix('#') {
            Some(rest.trim_start_matches('#'))
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            Some(rest)
        } else if let Some(rest) = trimmed.strip_prefix("* ") {
            Some(rest)
        } else if let Some(rest) = trimmed.strip_prefix("+ ") {
            Some(rest)
        } else {
            let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
            trimmed
                .get(digits..)
                .and_then(|rest| rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")))
        };
        match next {
            Some(rest) => line = rest,
            None => return trimmed,
        }
    }
}

/// Keep visible link text while dropping destinations, image markup, emphasis,
/// and HTML tags. This is deliberately a small preview cleaner, not a renderer.
fn clean_inline_markup(line: &str) -> String {
    let mut output = String::new();
    let mut remaining = line;
    while !remaining.is_empty() {
        if remaining.starts_with("![") {
            if let Some(end) = markdown_link_end(&remaining[1..]) {
                remaining = &remaining[end + 1..];
                continue;
            }
        }
        if remaining.starts_with('[') {
            if let Some(close) = remaining.find(']') {
                if remaining[close + 1..].starts_with('(') {
                    if let Some(end) = markdown_link_end(remaining) {
                        output.push_str(&remaining[1..close]);
                        remaining = &remaining[end..];
                        continue;
                    }
                }
            }
        }
        if remaining.starts_with('<') {
            if let Some(close) = remaining.find('>') {
                remaining = &remaining[close + 1..];
                continue;
            }
        }
        let character = remaining.chars().next().expect("nonempty remainder");
        remaining = &remaining[character.len_utf8()..];
        if character == '\\' {
            if let Some(next) = remaining.chars().next() {
                output.push(next);
                remaining = &remaining[next.len_utf8()..];
            }
        } else if !matches!(character, '*' | '_' | '~' | '`') {
            output.push(character);
        }
    }
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Byte length of `[label](destination)` from its opening bracket. Destinations
/// may include one nested parenthesis, as in local asset names.
fn markdown_link_end(value: &str) -> Option<usize> {
    let close = value.find("](")?;
    let mut depth = 1;
    for (offset, character) in value[close + 2..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(close + 2 + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn limit_description(value: String) -> String {
    let mut result = value
        .chars()
        .take(MAX_DESCRIPTION_CHARS)
        .collect::<String>();
    if result.chars().count() == MAX_DESCRIPTION_CHARS {
        if let Some(index) = result.rfind(char::is_whitespace) {
            result.truncate(index);
        }
    }
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(title: &str, excerpt: Option<&str>, body: &str) -> ArticleCardProjection {
        project_article_card(
            ReadingKind::Article,
            false,
            None,
            title,
            excerpt,
            None,
            body,
        )
    }

    #[test]
    fn keeps_saved_description_when_it_adds_information() {
        let card = project(
            "A heading",
            Some("A separate description"),
            "Body text starts here.",
        );
        assert_eq!(card.description.as_deref(), Some("A separate description"));
    }

    #[test]
    fn derives_description_from_body_when_excerpt_is_missing_or_repeats_heading() {
        let body = "# A heading\n\n![Cover](assets/cover.png)\n\nThe *first* [paragraph](https://example.com) begins here.\n\nAnd continues.";
        for excerpt in [None, Some(" "), Some("A heading.")] {
            let card = project("A heading", excerpt, body);
            assert_eq!(
                card.description.as_deref(),
                Some("The first paragraph begins here. And continues.")
            );
        }
    }

    #[test]
    fn does_not_invent_description_for_heading_and_images_only() {
        let card = project(
            "A heading",
            None,
            "# A heading\n\n![Cover](assets/cover.png)",
        );
        assert_eq!(card.description, None);
    }

    #[test]
    fn leaves_other_card_kinds_and_social_posts_without_card_description() {
        for (kind, lightweight, source_type) in [
            (ReadingKind::Image, false, None),
            (ReadingKind::Article, true, None),
            (ReadingKind::Article, false, Some("social_post")),
        ] {
            let card = project_article_card(
                kind,
                lightweight,
                source_type,
                "Title",
                None,
                Some(8),
                "Body",
            );
            assert_eq!(card.description, None);
            assert_eq!(card.word_count, Some(8));
        }
    }

    #[test]
    fn estimates_missing_word_count_only_for_full_articles() {
        assert_eq!(
            project("Title", None, "Three real body words.").word_count,
            Some(4)
        );
        assert_eq!(project("Title", None, "").word_count, None);
        assert_eq!(
            project_article_card(
                ReadingKind::Article,
                false,
                None,
                "Title",
                None,
                Some(0),
                "Article body"
            )
            .word_count,
            Some(2)
        );
        assert_eq!(
            project_article_card(
                ReadingKind::Article,
                false,
                None,
                "Title",
                None,
                Some(25),
                "Body"
            )
            .word_count,
            Some(25)
        );
    }

    #[test]
    fn bounds_description_and_ignores_code_fences() {
        let body = format!(
            "```rust\n{}\n```\n\n{}",
            "code ".repeat(3_000),
            "word ".repeat(500)
        );
        let card = project("Title", None, &body);
        let description = card.description.expect("body has prose");
        assert!(description.starts_with("word word"));
        assert!(description.chars().count() <= MAX_DESCRIPTION_CHARS);
    }
}
