// SPDX-License-Identifier: MIT

//! Read the small data-only subset of X's public server-rendered note results.
//! Serialized references are recognized only when their object is supplied
//! inline. This is deliberately not a JavaScript evaluator or general parser.

use std::collections::HashMap;

use base64::{engine::general_purpose::STANDARD, Engine};
use scraper::{Html, Selector};

use super::{decimal_identifier, XSourceError};

const HTML_LIMIT: usize = 2 * 1024 * 1024;
const MAX_OBJECT_DEPTH: usize = 64;
const MAX_OBJECTS: usize = 16_384;
const MAX_FIELDS: usize = 128;

pub(super) fn graphql_identifier(value: &str, kind: &str) -> Option<String> {
    if value.len() > 128 {
        return None;
    }
    let decoded = String::from_utf8(STANDARD.decode(value).ok()?).ok()?;
    let identifier = decoded.strip_prefix(kind)?.strip_prefix(':')?;
    decimal_identifier(identifier).then(|| identifier.to_owned())
}

pub(super) fn full_note_text(
    html: &str,
    note_results_id: &str,
    post_id: &str,
    preview: &str,
) -> Result<String, XSourceError> {
    let invalid = || XSourceError::MalformedPayload("complete note_tweet text");
    if html.len() > HTML_LIMIT || preview.is_empty() {
        return Err(invalid());
    }
    let note_id = graphql_identifier(note_results_id, "NoteTweetResults").ok_or_else(invalid)?;
    let document = Html::parse_document(html);
    let scripts = Selector::parse("script").expect("static selector");
    let mut complete: Option<String> = None;
    for script in document.select(&scripts) {
        let source = script.text().collect::<String>();
        if !source.contains("note_tweet_results") {
            continue;
        }
        let objects = serialized_objects(&source).map_err(|_| invalid())?;
        for object in &objects {
            let Some(results_index) = object.object("note_tweet_results") else {
                continue;
            };
            let results = &objects[results_index];
            if results.string("id") != Some(note_results_id) {
                continue;
            }
            if !object.valid || !results.valid {
                return Err(invalid());
            }
            let result = results
                .object("result")
                .map(|index| &objects[index])
                .ok_or_else(invalid)?;
            if !result.valid
                || result.string("__typename") != Some("NoteTweet")
                || result
                    .string("id")
                    .and_then(|id| graphql_identifier(id, "NoteTweet"))
                    .as_deref()
                    != Some(note_id.as_str())
                || !result.optional_identity("rest_id", &note_id)
            {
                return Err(invalid());
            }
            // Every accepted note requires a verified enclosing Tweet. Reject
            // unowned results, replies, or another post's serialized data.
            let mut ancestor = object.parent;
            let mut has_owner = false;
            while let Some(index) = ancestor {
                let owner = &objects[index];
                if owner.string("__typename") == Some("Tweet") {
                    if !owner.valid
                        || owner
                            .string("id")
                            .and_then(|id| graphql_identifier(id, "Tweet"))
                            .as_deref()
                            != Some(post_id)
                        || !owner.optional_identity("rest_id", post_id)
                    {
                        return Err(invalid());
                    }
                    has_owner = true;
                }
                ancestor = owner.parent;
            }
            let text = result.string("text").ok_or_else(invalid)?;
            // Short Note Tweets can already be complete in syndication. The
            // verified owning Tweet and note result still establish the full
            // text even when no additional characters need to be recovered.
            if !has_owner
                || !text.starts_with(preview)
                || text.trim_end().ends_with(['…'])
                || text.trim_end().ends_with("...")
                || complete.as_ref().is_some_and(|previous| previous != text)
            {
                return Err(invalid());
            }
            complete = Some(text.to_owned());
        }
    }
    complete.ok_or(XSourceError::TruncatedPost)
}

enum DataValue {
    String(String),
    Object(usize),
    Other,
}

struct DataObject {
    start: usize,
    end: usize,
    parent: Option<usize>,
    fields: HashMap<String, DataValue>,
    valid: bool,
}

impl DataObject {
    fn string(&self, field: &str) -> Option<&str> {
        match self.fields.get(field)? {
            DataValue::String(value) => Some(value),
            _ => None,
        }
    }

    fn object(&self, field: &str) -> Option<usize> {
        match self.fields.get(field)? {
            DataValue::Object(index) => Some(*index),
            _ => None,
        }
    }

    fn optional_identity(&self, field: &str, expected: &str) -> bool {
        !self.fields.contains_key(field) || self.string(field) == Some(expected)
    }
}

fn serialized_objects(source: &str) -> Result<Vec<DataObject>, ()> {
    let bytes = source.as_bytes();
    let mut objects = Vec::<DataObject>::new();
    let mut stack = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' | b'\'' | b'`' => cursor = quoted_end(bytes, cursor)?,
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor += 2;
                while cursor < bytes.len() && bytes[cursor] != b'\n' {
                    cursor += 1;
                }
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor += 2;
                while cursor + 1 < bytes.len() && &bytes[cursor..cursor + 2] != b"*/" {
                    cursor += 1;
                }
                if cursor + 1 >= bytes.len() {
                    return Err(());
                }
                cursor += 2;
            }
            b'{' => {
                if stack.len() >= MAX_OBJECT_DEPTH || objects.len() >= MAX_OBJECTS {
                    return Err(());
                }
                objects.push(DataObject {
                    start: cursor,
                    end: 0,
                    parent: stack.last().copied(),
                    fields: HashMap::new(),
                    valid: true,
                });
                stack.push(objects.len() - 1);
                cursor += 1;
            }
            b'}' => {
                let index = stack.pop().ok_or(())?;
                objects[index].end = cursor;
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }
    if !stack.is_empty() {
        return Err(());
    }
    let by_start: HashMap<_, _> = objects
        .iter()
        .enumerate()
        .map(|(index, object)| (object.start, (index, object.end)))
        .collect();
    for object in &mut objects {
        read_fields(source, object, &by_start);
    }
    Ok(objects)
}

fn quoted_end(bytes: &[u8], start: usize) -> Result<usize, ()> {
    let quote = bytes[start];
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor += 2;
        } else if bytes[cursor] == quote {
            return Ok(cursor + 1);
        } else {
            cursor += 1;
        }
    }
    Err(())
}

fn read_fields(source: &str, object: &mut DataObject, by_start: &HashMap<usize, (usize, usize)>) {
    let mut cursor = object.start + 1;
    let bytes = source.as_bytes();
    while cursor < object.end {
        while cursor < object.end && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor == object.end {
            break;
        }
        let key_start = cursor;
        let key = if bytes[cursor] == b'"' {
            let Ok(end) = quoted_end(bytes, cursor) else {
                object.valid = false;
                break;
            };
            cursor = end;
            serde_json::from_str::<String>(&source[key_start..end]).ok()
        } else {
            while cursor < object.end
                && (bytes[cursor].is_ascii_alphanumeric() || matches!(bytes[cursor], b'_' | b'$'))
            {
                cursor += 1;
            }
            (cursor > key_start).then(|| source[key_start..cursor].to_owned())
        };
        while cursor < object.end && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let Some(key) = key.filter(|_| bytes.get(cursor) == Some(&b':')) else {
            object.valid = false;
            break;
        };
        cursor += 1;
        let value_start = cursor;
        let mut array_depth = 0;
        while cursor < object.end {
            match bytes[cursor] {
                b'"' | b'\'' | b'`' => match quoted_end(bytes, cursor) {
                    Ok(end) => cursor = end,
                    Err(()) => {
                        object.valid = false;
                        return;
                    }
                },
                b'{' => match by_start.get(&cursor) {
                    Some((_, end)) => cursor = end + 1,
                    None => {
                        object.valid = false;
                        return;
                    }
                },
                b'[' => {
                    array_depth += 1;
                    cursor += 1;
                }
                b']' if array_depth > 0 => {
                    array_depth -= 1;
                    cursor += 1;
                }
                b',' if array_depth == 0 => break,
                _ => cursor += 1,
            }
        }
        if array_depth != 0 {
            object.valid = false;
            return;
        }
        if object.fields.len() >= MAX_FIELDS {
            object.valid = false;
            return;
        }
        let value_source = source[value_start..cursor].trim();
        let literal = inline_value(value_source);
        let value = if literal.starts_with('"') {
            match serde_json::from_str::<String>(literal) {
                Ok(value) => DataValue::String(value),
                Err(_) => {
                    object.valid = false;
                    return;
                }
            }
        } else if literal.starts_with('{') {
            let start = value_start + source[value_start..cursor].find(literal).unwrap_or(0);
            match by_start.get(&start) {
                Some((index, end)) if source[*end + 1..cursor].trim().is_empty() => {
                    DataValue::Object(*index)
                }
                _ => DataValue::Other,
            }
        } else {
            DataValue::Other
        };
        if object.fields.insert(key, value).is_some() {
            object.valid = false;
        }
        cursor += usize::from(cursor < object.end);
    }
}

fn inline_value(value: &str) -> &str {
    let Some(reference) = value.strip_prefix("$R[") else {
        return value;
    };
    let Some((index, assigned)) = reference.split_once("]=") else {
        return value;
    };
    if index.bytes().all(|byte| byte.is_ascii_digit()) && !index.is_empty() {
        assigned.trim_start()
    } else {
        value
    }
}
