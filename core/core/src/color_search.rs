// SPDX-License-Identifier: MIT

//! Palette search shared by the inspector and every list/count query.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;

use crate::WeightedColor;

/// A visible, editable search expression. Keeping this in the ordinary query
/// means scope changes and clearing the search all work alike.
pub(crate) fn query(color: &WeightedColor) -> String {
    format!("colour:{}", hex(color))
}

pub(crate) fn hex(color: &WeightedColor) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        (color.red * 255.0).round() as u8,
        (color.green * 255.0).round() as u8,
        (color.blue * 255.0).round() as u8
    )
}

pub(crate) fn parse(query: &str) -> Option<WeightedColor> {
    let lower = query.trim().to_ascii_lowercase();
    let hex = lower
        .strip_prefix("colour:#")
        .or_else(|| lower.strip_prefix("color:#"))
        .or_else(|| lower.strip_prefix('#'))?;
    if hex.len() != 6 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(WeightedColor {
        red: ((rgb >> 16) & 255) as f64 / 255.0,
        green: ((rgb >> 8) & 255) as f64 / 255.0,
        blue: (rgb & 255) as f64 / 255.0,
        weight: 1.0,
    })
}

/// Only the analysis version bound to the current reading can match. Optional
/// platform semantic results must not widen an explicit colour query.
pub(crate) fn matching_ids(conn: &Connection, color: &WeightedColor) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, a.palette_json FROM readings r
         JOIN visual_analysis a ON a.content_hash=r.visual_asset_hash
           AND a.analyzer_version=r.visual_analyzer_version
         WHERE a.supported=1",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut matches = Vec::new();
    for row in rows {
        let (id, json) = row?;
        let Ok(palette) = serde_json::from_str::<Vec<WeightedColor>>(&json) else {
            continue;
        };
        let Ok(palette) = crate::visual_index::normalize_palette(&palette) else {
            continue;
        };
        // Ignore tiny clusters (compression noise, isolated highlights). This
        // is also the inspector's minimum swatch coverage.
        if let Some(distance) = palette
            .iter()
            .filter(|sample| sample.weight >= MIN_COVERAGE)
            .map(|sample| distance(color, sample))
            .min_by(f64::total_cmp)
            .filter(|distance| *distance <= SIMILARITY_LIMIT)
        {
            matches.push((id, distance));
        }
    }
    matches.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    Ok(matches.into_iter().map(|(id, _)| id).collect())
}

/// Each selected swatch must match the same reading. Preserve the first
/// palette's distance ordering while intersecting subsequent swatches.
pub(crate) fn matching_ids_for_terms(
    conn: &Connection,
    terms: &[String],
) -> Result<Option<Vec<String>>> {
    let mut matches: Option<Vec<String>> = None;
    for term in terms {
        let Some(color) = parse(&format!("colour:{term}")) else {
            return Ok(Some(Vec::new()));
        };
        let ids = matching_ids(conn, &color)?;
        match &mut matches {
            Some(existing) => {
                let ids: HashSet<_> = ids.into_iter().collect();
                existing.retain(|id| ids.contains(id));
            }
            None => matches = Some(ids),
        }
        if matches.as_ref().is_some_and(Vec::is_empty) {
            break;
        }
    }
    Ok(matches)
}

pub(crate) const MIN_COVERAGE: f64 = 0.03;
// A deliberate product tolerance, not a claim of perceptual indistinguishability.
const SIMILARITY_LIMIT: f64 = 0.08;

pub(crate) fn distance(a: &WeightedColor, b: &WeightedColor) -> f64 {
    let a = oklab(a);
    let b = oklab(b);
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}

// Björn Ottosson's linear-sRGB → Oklab transform:
// https://bottosson.github.io/posts/oklab/
fn oklab(color: &WeightedColor) -> [f64; 3] {
    let linear = |channel: f64| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    let r = linear(color.red);
    let g = linear(color.green);
    let b = linear(color.blue);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CountScope, ListOptions, SortField};

    #[test]
    fn shade_search_ranks_matches_and_keeps_counts_and_scopes_consistent() {
        let temp = tempfile::TempDir::new().unwrap();
        let conn = crate::open_index(&temp.path().join("index.db")).unwrap();
        for (id, value, kind, tag) in [
            ("exact", "#BCA98E", "image", "room"),
            ("near", "#B0A087", "image", "room"),
            ("dark", "#381A0F", "image", "room"),
            ("blue", "#122ACF", "image", "room"),
            ("article", "#BCA98E", "article", "other"),
        ] {
            let entries = serde_json::json!([{"name": tag, "key": tag, "origin": "user"}]);
            conn.execute("INSERT INTO readings (id,title,url,canonical_url,saved_at,source_hash,tags_json,tag_entries_json,kind,visual_asset_hash,visual_analyzer_version) VALUES (?1,?1,?1,?1,'2026-09-25','',?2,?3,?4,?1,'test')", rusqlite::params![id, format!("[\"{tag}\"]"), entries.to_string(), kind]).unwrap();
            let palette =
                serde_json::to_string(&vec![parse(&format!("colour:{value}")).unwrap()]).unwrap();
            conn.execute("INSERT INTO visual_analysis (content_hash,analyzer_version,supported,labels_json,palette_json,visual_terms,completed_at) VALUES (?1,'test',1,'[]',?2,'','2026-09-25')", rusqlite::params![id,palette]).unwrap();
        }
        let mut options = ListOptions {
            query: Some("colour:#BCA98E".into()),
            sort: SortField::Relevance,
            tag: Some("room".into()),
            semantic_candidate_ids: vec!["blue".into()],
            ..Default::default()
        };
        let rows = crate::list_readings(&conn, &options).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["exact", "near"]
        );
        let counts = crate::view_counts(
            &conn,
            &CountScope {
                query: options.query.clone(),
                tag: options.tag.clone(),
                semantic_candidate_ids: options.semantic_candidate_ids.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(counts.all, 2);
        options.limit = 1;
        options.offset = 1;
        assert_eq!(crate::list_readings(&conn, &options).unwrap()[0].id, "near");
        conn.execute(
            "UPDATE readings SET visual_analyzer_version='new' WHERE id='exact'",
            [],
        )
        .unwrap();
        options.offset = 0;
        assert_eq!(crate::list_readings(&conn, &options).unwrap()[0].id, "near");
        conn.execute(
            "UPDATE visual_analysis SET supported=0 WHERE content_hash='near'",
            [],
        )
        .unwrap();
        assert!(crate::list_readings(&conn, &options).unwrap().is_empty());
    }

    #[test]
    fn only_complete_colour_expressions_are_reserved() {
        assert!(parse(" Color:#bcA98e ").is_some());
        assert_eq!(parse(" #bca98e "), parse("colour:#BCA98E"));
        for invalid in [
            "#123",
            "#zzffff",
            "#123456 room",
            "colour:red",
            "colour:#123",
            "colour:#zzffff",
            "colour:#123456 room",
            "chair",
        ] {
            assert!(parse(invalid).is_none());
        }
    }

    #[test]
    fn bare_hex_search_uses_palette_matching() {
        let temp = tempfile::TempDir::new().unwrap();
        let conn = crate::open_index(&temp.path().join("index.db")).unwrap();
        for (id, value) in [("red", "#FF0000"), ("blue", "#0000FF")] {
            conn.execute(
                "INSERT INTO readings (id,title,url,canonical_url,saved_at,source_hash,kind,visual_asset_hash,visual_analyzer_version) VALUES (?1,?1,?1,?1,'2026-09-25','','image',?1,'test')",
                [id],
            ).unwrap();
            let palette = serde_json::to_string(&vec![parse(value).unwrap()]).unwrap();
            conn.execute(
                "INSERT INTO visual_analysis (content_hash,analyzer_version,supported,labels_json,palette_json,visual_terms,completed_at) VALUES (?1,'test',1,'[]',?2,'','2026-09-25')",
                rusqlite::params![id, palette],
            ).unwrap();
        }
        let options = ListOptions {
            query: Some("#ff0000".into()),
            sort: SortField::Relevance,
            ..Default::default()
        };
        let rows = crate::list_readings(&conn, &options).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["red"]
        );
    }

    #[test]
    fn color_tokens_filter_text_results_and_intersect() {
        let temp = tempfile::TempDir::new().unwrap();
        let conn = crate::open_index(&temp.path().join("index.db")).unwrap();
        for (id, color) in [("green", "#42C878"), ("blue", "#1234DB")] {
            conn.execute(
                "INSERT INTO readings (id,title,url,canonical_url,saved_at,source_hash,kind,visual_asset_hash,visual_analyzer_version) VALUES (?1,'Chair',?1,?1,'2026-09-25','','image',?1,'test')",
                [id],
            ).unwrap();
            let palette =
                serde_json::to_string(&vec![parse(&format!("colour:{color}")).unwrap()]).unwrap();
            conn.execute(
                "INSERT INTO visual_analysis (content_hash,analyzer_version,supported,labels_json,palette_json,visual_terms,completed_at) VALUES (?1,'test',1,'[]',?2,'','2026-09-25')",
                rusqlite::params![id, palette],
            ).unwrap();
        }
        let options = ListOptions {
            query: Some("chair".into()),
            color_terms: vec!["#42C878".into()],
            sort: SortField::Relevance,
            ..Default::default()
        };
        let rows = crate::list_readings(&conn, &options).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["green"]
        );

        let color_only = ListOptions {
            color_terms: vec!["#42C878".into()],
            item_type_terms: vec!["image".into()],
            ..Default::default()
        };
        let rows = crate::list_readings(&conn, &color_only).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["green"]
        );

        let wrong_type = ListOptions {
            color_terms: vec!["#42C878".into()],
            item_type_terms: vec!["quote".into()],
            ..Default::default()
        };
        assert!(crate::list_readings(&conn, &wrong_type).unwrap().is_empty());

        let no_match = ListOptions {
            color_terms: vec!["#42C878".into(), "#1234DB".into()],
            ..Default::default()
        };
        assert!(crate::list_readings(&conn, &no_match).unwrap().is_empty());
    }
}
