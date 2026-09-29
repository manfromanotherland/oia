// SPDX-License-Identifier: MIT

//! On-demand inspector facts. Presentation never receives an internal asset URL.

use anyhow::Result;
use nom_exif::{EntryValue, MediaParser, MediaSource, TrackInfoTag};
use rusqlite::{Connection, OptionalExtension};
use std::io::{Read, Seek, SeekFrom};

use crate::{
    color_search, list::ReadingRow, visual_index, LibraryRoot, ReadingKind, VisualLabel,
    WeightedColor,
};

#[derive(Debug, uniffi::Record)]
pub struct ReadingInspector {
    pub labels: Vec<String>,
    pub colors: Vec<InspectorColor>,
    pub analysis_available: bool,
    pub file: Option<InspectorFile>,
    pub has_local_file: bool,
}

#[derive(Debug, uniffi::Record)]
pub struct InspectorColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub hex: String,
    pub search_query: String,
}

#[derive(Debug, uniffi::Record)]
pub struct InspectorFile {
    pub format: String,
    pub byte_count: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub codecs: Vec<String>,
    pub color_profile: Option<String>,
}

pub(crate) struct Snapshot {
    row: ReadingRow,
    analysis: Option<(String, bool, String, String)>,
}

impl Snapshot {
    pub(crate) fn read(conn: &Connection, id: &str) -> Result<Option<Self>> {
        let Some((row, _)) = crate::get_reading(conn, id)? else {
            return Ok(None);
        };
        let analysis = conn.query_row(
            "SELECT a.content_hash, a.supported, a.labels_json, a.palette_json
             FROM readings r JOIN visual_analysis a
               ON a.content_hash=r.visual_asset_hash AND a.analyzer_version=r.visual_analyzer_version
             WHERE r.id=?1", [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).optional()?;
        Ok(Some(Self { row, analysis }))
    }

    /// Filesystem work happens after releasing the database lock. Cached labels
    /// are only displayed when they describe the actual preview bytes on disk.
    pub(crate) fn inspect(self, library: &LibraryRoot) -> ReadingInspector {
        let row = &self.row;
        let media = row
            .media_url
            .as_deref()
            .and_then(|url| url.strip_prefix("cuttings-asset:"));
        let path = media.or(row.preview_asset.as_deref());
        let file = path.and_then(|path| file_facts(library, row, path, media.is_some()));
        let mut result = ReadingInspector {
            labels: Vec::new(),
            colors: Vec::new(),
            analysis_available: false,
            file,
            has_local_file: path.is_some(),
        };
        let Some((hash, supported, labels, palette)) = self.analysis else {
            return result;
        };
        let current = row
            .preview_asset
            .as_deref()
            .and_then(|path| visual_index::inspect_asset(library, &row.id, path).ok());
        if !current.is_some_and(|asset| asset.content_hash == hash) {
            return result;
        }
        result.analysis_available = true;
        if supported {
            result.labels = display_labels(&labels);
            result.colors = display_colors(&palette);
        }
        result
    }
}

fn file_facts(
    library: &LibraryRoot,
    row: &ReadingRow,
    path: &str,
    is_media: bool,
) -> Option<InspectorFile> {
    let binding = if is_media {
        visual_index::AssetBinding::Media(row.media_url.as_deref()?)
    } else {
        visual_index::AssetBinding::Preview(path)
    };
    let mut file = visual_index::open_bound_asset(library, &row.id, path, binding).ok()?;
    let byte_count = file.metadata().ok()?.len();
    let dimensions = if is_media && row.kind == ReadingKind::Video {
        crate::media_dimensions::video_dimensions(&mut file)
    } else {
        crate::media_dimensions::image_dimensions(&mut file)
    };
    let (duration_ms, codecs, color_profile) = if is_media && row.kind == ReadingKind::Video {
        let duration_ms = video_duration_ms(&mut file);
        let (codecs, color_profile) = iso_video_facts(&mut file);
        (duration_ms, codecs, color_profile)
    } else {
        (
            None,
            Vec::new(),
            png_color_profile(&mut file).or_else(|| jpeg_icc_profile(&mut file)),
        )
    };
    let extension = std::path::Path::new(path)
        .extension()?
        .to_str()?
        .to_ascii_uppercase();
    let format = match extension.as_str() {
        "JPG" | "JPEG" => "JPEG".to_owned(),
        "TIF" | "TIFF" => "TIFF".to_owned(),
        _ => extension,
    };
    Some(InspectorFile {
        format,
        byte_count,
        width: dimensions.map(|d| d.width),
        height: dimensions.map(|d| d.height),
        duration_ms,
        codecs,
        color_profile,
    })
}

fn video_duration_ms<R: Read + Seek>(reader: &mut R) -> Option<u64> {
    reader.seek(SeekFrom::Start(0)).ok()?;
    let source = MediaSource::seekable(reader).ok()?;
    let mut parser = MediaParser::new();
    let track = parser.parse_track(source).ok()?;
    match track.get(TrackInfoTag::DurationMs)? {
        EntryValue::U64(value) if *value > 0 => Some(*value),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct IsoBox {
    kind: [u8; 4],
    body_start: u64,
    end: u64,
}

/// Seek through the ISO BMFF box tree without loading movie bytes. A malformed
/// size must never escape its parent or cause an unbounded scan.
fn next_iso_box<R: Read + Seek>(
    reader: &mut R,
    cursor: &mut u64,
    parent_end: u64,
    budget: &mut usize,
) -> Option<IsoBox> {
    if *budget == 0 || cursor.checked_add(8)? > parent_end {
        return None;
    }
    *budget -= 1;
    reader.seek(SeekFrom::Start(*cursor)).ok()?;
    let mut header = [0; 8];
    reader.read_exact(&mut header).ok()?;
    let size32 = u32::from_be_bytes(header[..4].try_into().ok()?) as u64;
    let mut header_size = 8_u64;
    let size = match size32 {
        0 => parent_end.checked_sub(*cursor)?,
        1 => {
            if cursor.checked_add(16)? > parent_end {
                return None;
            }
            let mut extended = [0; 8];
            reader.read_exact(&mut extended).ok()?;
            header_size = 16;
            u64::from_be_bytes(extended)
        }
        _ => size32,
    };
    if size < header_size {
        return None;
    }
    let end = cursor.checked_add(size)?;
    if end > parent_end {
        return None;
    }
    let result = IsoBox {
        kind: header[4..8].try_into().ok()?,
        body_start: cursor.checked_add(header_size)?,
        end,
    };
    *cursor = end;
    Some(result)
}

fn iso_child<R: Read + Seek>(
    reader: &mut R,
    parent: IsoBox,
    kind: [u8; 4],
    budget: &mut usize,
) -> Option<IsoBox> {
    let mut cursor = parent.body_start;
    while let Some(child) = next_iso_box(reader, &mut cursor, parent.end, budget) {
        if child.kind == kind {
            return Some(child);
        }
    }
    None
}

fn iso_video_facts<R: Read + Seek>(reader: &mut R) -> (Vec<String>, Option<String>) {
    let mut codecs = Vec::new();
    let mut color_profile = None;
    let Some(file_end) = reader.seek(SeekFrom::End(0)).ok() else {
        return (codecs, color_profile);
    };
    let mut budget = 4096;
    let root = IsoBox {
        kind: *b"root",
        body_start: 0,
        end: file_end,
    };
    let Some(moov) = iso_child(reader, root, *b"moov", &mut budget) else {
        return (codecs, color_profile);
    };
    let mut cursor = moov.body_start;
    while let Some(track) = next_iso_box(reader, &mut cursor, moov.end, &mut budget) {
        if track.kind != *b"trak" {
            continue;
        }
        let Some(media) = iso_child(reader, track, *b"mdia", &mut budget) else {
            continue;
        };
        let Some(minf) = iso_child(reader, media, *b"minf", &mut budget) else {
            continue;
        };
        let Some(stbl) = iso_child(reader, minf, *b"stbl", &mut budget) else {
            continue;
        };
        let Some(stsd) = iso_child(reader, stbl, *b"stsd", &mut budget) else {
            continue;
        };
        let Some(entries_start) = stsd.body_start.checked_add(8) else {
            continue;
        };
        if entries_start > stsd.end {
            continue;
        }
        if reader.seek(SeekFrom::Start(stsd.body_start + 4)).is_err() {
            continue;
        }
        let mut count_bytes = [0; 4];
        if reader.read_exact(&mut count_bytes).is_err() {
            continue;
        }
        let count = u32::from_be_bytes(count_bytes);
        if count == 0 || count > 32 {
            continue;
        }
        let mut entry_cursor = entries_start;
        for _ in 0..count {
            let Some(entry) = next_iso_box(reader, &mut entry_cursor, stsd.end, &mut budget) else {
                break;
            };
            if let Some(codec) = sample_entry_codec(entry.kind) {
                if !codecs.iter().any(|existing| existing == codec) {
                    codecs.push(codec.to_owned());
                }
            }
            if color_profile.is_none() && is_video_sample_entry(entry.kind) {
                color_profile = video_sample_color_profile(reader, entry, &mut budget);
            }
        }
    }
    (codecs, color_profile)
}

fn sample_entry_codec(kind: [u8; 4]) -> Option<&'static str> {
    match &kind {
        b"avc1" | b"avc3" => Some("H.264"),
        b"hvc1" | b"hev1" => Some("HEVC"),
        b"av01" => Some("AV1"),
        b"vp09" => Some("VP9"),
        b"mp4v" => Some("MPEG-4 video"),
        b"mp4a" => Some("MPEG-4 audio"),
        b"ac-3" => Some("Dolby Digital"),
        b"ec-3" => Some("Dolby Digital Plus"),
        b"alac" => Some("Apple Lossless"),
        b"Opus" => Some("Opus"),
        b"fLaC" => Some("FLAC"),
        _ => None,
    }
}

fn is_video_sample_entry(kind: [u8; 4]) -> bool {
    matches!(
        &kind,
        b"avc1" | b"avc3" | b"hvc1" | b"hev1" | b"av01" | b"vp09" | b"mp4v"
    )
}

fn video_sample_color_profile<R: Read + Seek>(
    reader: &mut R,
    entry: IsoBox,
    budget: &mut usize,
) -> Option<String> {
    // VisualSampleEntry has a fixed 78-byte header after its own box header.
    let mut cursor = entry.body_start.checked_add(78)?;
    if cursor > entry.end {
        return None;
    }
    while let Some(child) = next_iso_box(reader, &mut cursor, entry.end, budget) {
        if child.kind != *b"colr" || child.body_start.checked_add(10)? > child.end {
            continue;
        }
        reader.seek(SeekFrom::Start(child.body_start)).ok()?;
        let mut values = [0; 10];
        reader.read_exact(&mut values).ok()?;
        let profile_type = &values[..4];
        if profile_type != b"nclx" && profile_type != b"nclc" {
            continue;
        }
        if profile_type == b"nclx" && child.body_start.checked_add(11)? > child.end {
            continue;
        }
        let primaries = u16::from_be_bytes(values[4..6].try_into().ok()?);
        let transfer = u16::from_be_bytes(values[6..8].try_into().ok()?);
        let matrix = u16::from_be_bytes(values[8..10].try_into().ok()?);
        if primaries == 1 && transfer == 1 && matrix == 1 {
            return Some("HD (1-1-1)".to_owned());
        }
        let label = if profile_type == b"nclx" {
            "NCLX"
        } else {
            "NCLC"
        };
        return Some(format!("{label} ({primaries}-{transfer}-{matrix})"));
    }
    None
}

fn png_color_profile<R: Read + Seek>(reader: &mut R) -> Option<String> {
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut signature = [0; 8];
    reader.read_exact(&mut signature).ok()?;
    if signature != *b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let file_end = reader.seek(SeekFrom::End(0)).ok()?;
    let mut cursor = 8_u64;
    for _ in 0..256 {
        if cursor.checked_add(12)? > file_end {
            return None;
        }
        reader.seek(SeekFrom::Start(cursor)).ok()?;
        let mut header = [0; 8];
        reader.read_exact(&mut header).ok()?;
        let length = u32::from_be_bytes(header[..4].try_into().ok()?) as u64;
        let data_start = cursor.checked_add(8)?;
        let next = data_start.checked_add(length)?.checked_add(4)?;
        if next > file_end {
            return None;
        }
        match &header[4..] {
            b"sRGB" if length == 1 => return Some("sRGB".to_owned()),
            b"iCCP" if (3..=1_048_576).contains(&length) => {
                let mut prefix = [0; 81];
                let read_len = usize::try_from(length.min(81)).ok()?;
                reader.read_exact(&mut prefix[..read_len]).ok()?;
                let name_end = prefix[..read_len].iter().position(|byte| *byte == 0)?;
                if !(1..=79).contains(&name_end)
                    || prefix.get(name_end + 1) != Some(&0)
                    || length < (name_end as u64 + 3)
                {
                    return None;
                }
                let name = prefix[..name_end]
                    .iter()
                    .map(|byte| char::from(*byte))
                    .collect::<String>();
                if name.chars().all(|character| !character.is_control()) {
                    return Some(name);
                }
                return None;
            }
            b"IDAT" | b"IEND" => return None,
            _ => {}
        }
        cursor = next;
    }
    None
}

fn jpeg_icc_profile<R: Read + Seek>(reader: &mut R) -> Option<String> {
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut signature = [0; 2];
    reader.read_exact(&mut signature).ok()?;
    if signature != [0xff, 0xd8] {
        return None;
    }
    let file_end = reader.seek(SeekFrom::End(0)).ok()?;
    let mut cursor = 2_u64;
    let mut segments: Vec<Option<Vec<u8>>> = Vec::new();
    for _ in 0..1024 {
        if cursor.checked_add(2)? > file_end {
            return None;
        }
        reader.seek(SeekFrom::Start(cursor)).ok()?;
        let mut marker = [0; 2];
        reader.read_exact(&mut marker).ok()?;
        if marker[0] != 0xff || marker[1] == 0xff {
            return None;
        }
        if marker[1] == 0xd9 || marker[1] == 0xda {
            break;
        }
        // Restart and stand-alone markers have no length and are not expected
        // before SOS, but advancing them keeps a malformed file finite.
        if marker[1] == 0x01 || (0xd0..=0xd8).contains(&marker[1]) {
            cursor += 2;
            continue;
        }
        if cursor.checked_add(4)? > file_end {
            return None;
        }
        let mut length_bytes = [0; 2];
        reader.read_exact(&mut length_bytes).ok()?;
        let length = u16::from_be_bytes(length_bytes) as u64;
        if length < 2 {
            return None;
        }
        let next = cursor.checked_add(2)?.checked_add(length)?;
        if next > file_end {
            return None;
        }
        if marker[1] == 0xe2 && length >= 16 {
            let mut prefix = [0; 14];
            reader.read_exact(&mut prefix).ok()?;
            if &prefix[..12] == b"ICC_PROFILE\0" {
                let sequence = prefix[12];
                let count = prefix[13];
                if count == 0 || count > 64 || sequence == 0 || sequence > count {
                    return None;
                }
                if segments.is_empty() {
                    segments.resize_with(count as usize, || None);
                } else if segments.len() != count as usize {
                    return None;
                }
                let slot = &mut segments[sequence as usize - 1];
                if slot.is_some() {
                    return None;
                }
                let payload_len = usize::try_from(length - 16).ok()?;
                let mut payload = vec![0; payload_len];
                reader.read_exact(&mut payload).ok()?;
                *slot = Some(payload);
            }
        }
        cursor = next;
    }
    if segments.is_empty() || segments.iter().any(Option::is_none) {
        return None;
    }
    let total = segments.iter().try_fold(0_usize, |total, segment| {
        total.checked_add(segment.as_ref()?.len())
    })?;
    if total > 4 * 1024 * 1024 {
        return None;
    }
    let mut profile = Vec::with_capacity(total);
    for segment in segments {
        profile.extend(segment?);
    }
    icc_description(&profile)
}

fn icc_description(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 132 || bytes.get(36..40)? != b"acsp" {
        return None;
    }
    let declared_len = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    if declared_len < 132 || declared_len > bytes.len() {
        return None;
    }
    let count = u32::from_be_bytes(bytes.get(128..132)?.try_into().ok()?) as usize;
    if count > 1024 || 132_usize.checked_add(count.checked_mul(12)?)? > declared_len {
        return None;
    }
    for index in 0..count {
        let entry = bytes.get(132 + index * 12..144 + index * 12)?;
        if &entry[..4] != b"desc" {
            continue;
        }
        let offset = u32::from_be_bytes(entry[4..8].try_into().ok()?) as usize;
        let length = u32::from_be_bytes(entry[8..12].try_into().ok()?) as usize;
        let end = offset.checked_add(length)?;
        if end > declared_len || length < 12 {
            return None;
        }
        let data = bytes.get(offset..end)?;
        return match &data[..4] {
            b"desc" => {
                let name_len = u32::from_be_bytes(data[8..12].try_into().ok()?) as usize;
                if name_len == 0 || name_len > 256 || 12_usize.checked_add(name_len)? > data.len() {
                    return None;
                }
                let name_bytes = data[12..12 + name_len]
                    .strip_suffix(&[0])
                    .unwrap_or(&data[12..12 + name_len]);
                if name_bytes.is_empty()
                    || !name_bytes.iter().all(|byte| (0x20..=0x7e).contains(byte))
                {
                    return None;
                }
                String::from_utf8(name_bytes.to_vec()).ok()
            }
            b"mluc" => {
                if data.len() < 28 {
                    return None;
                }
                let record_count = u32::from_be_bytes(data[8..12].try_into().ok()?) as usize;
                let record_size = u32::from_be_bytes(data[12..16].try_into().ok()?) as usize;
                if record_count == 0 || record_count > 64 || record_size != 12 {
                    return None;
                }
                for record_index in 0..record_count {
                    let start = 16_usize.checked_add(record_index.checked_mul(record_size)?)?;
                    let record = data.get(start..start + record_size)?;
                    let text_len = u32::from_be_bytes(record[4..8].try_into().ok()?) as usize;
                    let text_offset = u32::from_be_bytes(record[8..12].try_into().ok()?) as usize;
                    if text_len == 0 || text_len > 512 || !text_len.is_multiple_of(2) {
                        continue;
                    }
                    let Some(text) = text_offset
                        .checked_add(text_len)
                        .and_then(|end| data.get(text_offset..end))
                    else {
                        continue;
                    };
                    let utf16: Vec<u16> = text
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| u16::from_be_bytes(*pair))
                        .collect();
                    if let Ok(name) = String::from_utf16(&utf16) {
                        if !name.is_empty() && name.chars().all(|character| !character.is_control())
                        {
                            return Some(name);
                        }
                    }
                }
                None
            }
            _ => None,
        };
    }
    None
}

fn display_labels(json: &str) -> Vec<String> {
    let mut labels = serde_json::from_str::<Vec<VisualLabel>>(json).unwrap_or_default();
    labels.retain(|label| {
        label.confidence >= 0.25
            && label.confidence <= 1.0
            && !matches!(
                label.identifier.as_str(),
                "object" | "structure" | "conveyance" | "portal" | "material" | "decoration"
            )
    });
    labels.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.identifier.cmp(&b.identifier))
    });
    let mut seen = std::collections::HashSet::new();
    labels
        .into_iter()
        .map(|l| l.identifier)
        .filter(|l| !l.is_empty() && seen.insert(l.clone()))
        .take(6)
        .collect()
}

fn display_colors(json: &str) -> Vec<InspectorColor> {
    let palette = serde_json::from_str::<Vec<WeightedColor>>(json).unwrap_or_default();
    let palette = visual_index::normalize_palette(&palette).unwrap_or_default();
    let mut distinct: Vec<WeightedColor> = Vec::new();
    for color in palette
        .into_iter()
        .filter(|c| c.weight >= color_search::MIN_COVERAGE)
    {
        if distinct
            .iter()
            .all(|c| color_search::distance(c, &color) >= 0.035)
        {
            distinct.push(color);
        }
        if distinct.len() == 5 {
            break;
        }
    }
    distinct
        .into_iter()
        .map(|color| InspectorColor {
            red: color.red,
            green: color.green,
            blue: color.blue,
            hex: color_search::hex(&color),
            search_query: color_search::query(&color),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{import_image, open_index, rebuild, VisualAnalysisResult};
    use std::io::Cursor;
    use tempfile::TempDir;

    #[test]
    fn inspector_reads_facts_and_rejects_stale_or_escaped_assets() {
        let temp = TempDir::new().unwrap();
        let library = LibraryRoot::new(temp.path()).unwrap();
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82];
        png.extend(1280_u32.to_be_bytes());
        png.extend(1924_u32.to_be_bytes());
        png.extend([8, 2, 0, 0, 0]);
        let id = import_image(&library, png.clone(), "image/png", "Room")
            .unwrap()
            .id;
        let db_path = temp.path().join("index.db");
        let conn = open_index(&db_path).unwrap();
        rebuild(&conn, &library).unwrap();
        let cache = visual_index::prepare_visual_cache(&db_path).unwrap();
        let task = visual_index::pending_visual_analysis(&conn, &library, &cache, "test", 1)
            .unwrap()
            .tasks
            .remove(0);
        let analysis = VisualAnalysisResult {
            supported: true,
            labels: vec![
                VisualLabel {
                    identifier: "Cabinet".into(),
                    confidence: 0.9,
                },
                VisualLabel {
                    identifier: "structure".into(),
                    confidence: 0.99,
                },
                VisualLabel {
                    identifier: "noise".into(),
                    confidence: 0.1,
                },
            ],
            palette: vec![color_search::parse("colour:#BCA98E").unwrap()],
        };
        assert!(visual_index::complete_visual_analysis(&conn, &library, &task, &analysis).unwrap());
        let data = Snapshot::read(&conn, &id)
            .unwrap()
            .unwrap()
            .inspect(&library);
        assert_eq!(data.labels, ["cabinet"]);
        assert_eq!(data.colors[0].search_query, "colour:#BCA98E");
        let file = data.file.unwrap();
        assert_eq!((file.width, file.height), (Some(1280), Some(1924)));
        assert_eq!(file.byte_count, png.len() as u64);
        assert_eq!(file.format, "PNG");
        assert_eq!(file.duration_ms, None);
        assert!(file.codecs.is_empty());
        assert_eq!(file.color_profile, None);
        let asset_path = library.reading_dir(&id).join(&task.relative_path);
        std::fs::write(&asset_path, b"externally replaced").unwrap();
        let stale = Snapshot::read(&conn, &id)
            .unwrap()
            .unwrap()
            .inspect(&library);
        assert!(!stale.analysis_available);
        assert!(stale.colors.is_empty() && stale.labels.is_empty());
        std::fs::remove_file(&asset_path).unwrap();
        #[cfg(unix)]
        {
            let outside = temp.path().join("outside.png");
            std::fs::write(&outside, png).unwrap();
            std::os::unix::fs::symlink(outside, &asset_path).unwrap();
        }
        let missing = Snapshot::read(&conn, &id)
            .unwrap()
            .unwrap()
            .inspect(&library);
        assert!(missing.file.is_none());
        assert!(missing.has_local_file);
        assert!(!missing.analysis_available);
    }

    #[test]
    fn swatches_omit_noise_and_merge_near_duplicates() {
        let mut colors = vec![
            color_search::parse("color:#BCA98E").unwrap(),
            color_search::parse("color:#BDA98E").unwrap(),
            color_search::parse("color:#381A0F").unwrap(),
        ];
        colors[2].weight = 0.01;
        let palette = serde_json::to_string(&colors).unwrap();
        let swatches = display_colors(&palette);
        assert_eq!(swatches.len(), 1);
        assert!(display_colors("invalid").is_empty());
    }

    #[test]
    fn mp4_facts_come_from_sample_entries_and_color_box() {
        let mut video_entry = vec![0; 78];
        video_entry.extend(atom(*b"colr", b"nclx\0\x01\0\x01\0\x01\0".to_vec()));
        let video = sample_track(*b"avc1", video_entry);
        let audio = sample_track(*b"mp4a", vec![0; 28]);
        let mut moov_body = movie_header(3_250);
        moov_body.extend(video);
        moov_body.extend(audio);
        let mut bytes = atom(*b"ftyp", b"mp42\0\0\0\0mp42".to_vec());
        bytes.extend(atom(*b"moov", moov_body));
        let mut file = Cursor::new(bytes);

        assert_eq!(
            iso_video_facts(&mut file),
            (
                vec!["H.264".into(), "MPEG-4 audio".into()],
                Some("HD (1-1-1)".into())
            )
        );
        assert_eq!(video_duration_ms(&mut file), Some(3_250));
    }

    #[test]
    fn png_profile_and_malformed_headers_are_omitted_safely() {
        let mut srgb = b"\x89PNG\r\n\x1a\n".to_vec();
        srgb.extend(png_chunk(*b"sRGB", &[0]));
        assert_eq!(
            png_color_profile(&mut Cursor::new(srgb)),
            Some("sRGB".into())
        );

        let mut icc = b"\x89PNG\r\n\x1a\n".to_vec();
        icc.extend(png_chunk(*b"iCCP", b"Display P3\0\0compressed"));
        assert_eq!(
            png_color_profile(&mut Cursor::new(icc)),
            Some("Display P3".into())
        );

        let mut malformed_png = b"\x89PNG\r\n\x1a\n".to_vec();
        malformed_png.extend_from_slice(&u32::MAX.to_be_bytes());
        malformed_png.extend_from_slice(b"iCCP");
        assert_eq!(png_color_profile(&mut Cursor::new(malformed_png)), None);

        let mut malformed_mp4 = Vec::new();
        malformed_mp4.extend_from_slice(&u32::MAX.to_be_bytes());
        malformed_mp4.extend_from_slice(b"moov");
        assert_eq!(
            iso_video_facts(&mut Cursor::new(malformed_mp4)),
            (Vec::new(), None)
        );
    }

    #[test]
    fn jpeg_icc_description_handles_segments_and_rejects_unknown_profiles() {
        let profile = icc_with_description("sRGB IEC61966-2.1");
        let split = profile.len() / 2;
        let mut jpeg = vec![0xff, 0xd8];
        jpeg.extend(jpeg_app2_icc(&profile[split..], 2, 2));
        jpeg.extend(jpeg_app2_icc(&profile[..split], 1, 2));
        jpeg.extend([0xff, 0xd9]);
        assert_eq!(
            jpeg_icc_profile(&mut Cursor::new(jpeg)),
            Some("sRGB IEC61966-2.1".into())
        );

        let mut unknown_profile = profile;
        unknown_profile[132..136].copy_from_slice(b"cprt");
        let mut jpeg = vec![0xff, 0xd8];
        jpeg.extend(jpeg_app2_icc(&unknown_profile, 1, 1));
        jpeg.extend([0xff, 0xd9]);
        assert_eq!(jpeg_icc_profile(&mut Cursor::new(jpeg)), None);

        let mut incomplete = vec![0xff, 0xd8];
        incomplete.extend(jpeg_app2_icc(&unknown_profile, 1, 2));
        incomplete.extend([0xff, 0xd9]);
        assert_eq!(jpeg_icc_profile(&mut Cursor::new(incomplete)), None);
    }

    fn sample_track(kind: [u8; 4], entry_body: Vec<u8>) -> Vec<u8> {
        let mut stsd_body = vec![0; 4];
        stsd_body.extend_from_slice(&1_u32.to_be_bytes());
        stsd_body.extend(atom(kind, entry_body));
        atom(
            *b"trak",
            atom(
                *b"mdia",
                atom(*b"minf", atom(*b"stbl", atom(*b"stsd", stsd_body))),
            ),
        )
    }

    fn movie_header(duration_ms: u32) -> Vec<u8> {
        let mut body = vec![0; 12];
        body.extend_from_slice(&1_000_u32.to_be_bytes());
        body.extend_from_slice(&duration_ms.to_be_bytes());
        body.extend_from_slice(&[0; 76]);
        body.extend_from_slice(&1_u32.to_be_bytes());
        atom(*b"mvhd", body)
    }

    fn atom(kind: [u8; 4], body: Vec<u8>) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(body.len() + 8);
        bytes.extend_from_slice(&u32::try_from(body.len() + 8).unwrap().to_be_bytes());
        bytes.extend_from_slice(&kind);
        bytes.extend(body);
        bytes
    }

    fn png_chunk(kind: [u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(&kind);
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    fn icc_with_description(name: &str) -> Vec<u8> {
        let mut tag = b"desc\0\0\0\0".to_vec();
        tag.extend_from_slice(&u32::try_from(name.len() + 1).unwrap().to_be_bytes());
        tag.extend_from_slice(name.as_bytes());
        tag.push(0);
        let offset = 144_u32;
        let size = u32::try_from(tag.len()).unwrap();
        let mut profile = vec![0; offset as usize];
        profile[36..40].copy_from_slice(b"acsp");
        profile[128..132].copy_from_slice(&1_u32.to_be_bytes());
        profile[132..136].copy_from_slice(b"desc");
        profile[136..140].copy_from_slice(&offset.to_be_bytes());
        profile[140..144].copy_from_slice(&size.to_be_bytes());
        profile.extend(tag);
        let total = u32::try_from(profile.len()).unwrap();
        profile[..4].copy_from_slice(&total.to_be_bytes());
        profile
    }

    fn jpeg_app2_icc(profile_segment: &[u8], sequence: u8, count: u8) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xe2];
        bytes.extend_from_slice(
            &u16::try_from(profile_segment.len() + 16)
                .unwrap()
                .to_be_bytes(),
        );
        bytes.extend_from_slice(b"ICC_PROFILE\0");
        bytes.extend([sequence, count]);
        bytes.extend(profile_segment);
        bytes
    }
}
