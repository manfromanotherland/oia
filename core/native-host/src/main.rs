// SPDX-License-Identifier: MIT

mod check;
mod install;
mod protocol;
mod save;
mod save_link;
mod save_selection;
mod video_import;

use std::io::{Read, Write};

use anyhow::Result;
use protocol::{
    CheckRequest, SaveLinkRequest, SaveRequest, SaveResponse, SaveSelectionRequest,
    VideoImportBeginRequest, VideoImportChunkRequest, VideoImportEndRequest,
};
use serde::Deserialize;
use video_import::VideoImportSession;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version") {
        println!("oia-native-host v{}", oia_core::version());
        return Ok(());
    }

    if args.iter().any(|a| a == "--install-manifest") {
        let ext_id = args
            .windows(2)
            .find(|w| w[0] == "--extension-id")
            .map(|w| w[1].as_str());
        install::install_manifest(ext_id)?;
        return Ok(());
    }

    run_loop()
}

/// Read and handle native messaging messages until stdin closes.
fn run_loop() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    run_loop_with(&mut stdin.lock(), &mut stdout.lock())
}

/// Handle one browser connection. Video imports are scoped to this stream, so
/// EOF and transport errors drop the session and its incomplete staged file.
fn run_loop_with(reader: &mut impl Read, writer: &mut impl Write) -> Result<()> {
    let mut session = NativeSession::default();

    loop {
        let raw = match read_message(reader) {
            Ok(bytes) => bytes,
            Err(e) if is_eof_error(&e) => break,
            Err(e) => return Err(e),
        };

        let response = session.dispatch(&raw);
        let payload = serde_json::to_vec(&response)?;

        // Enforce the 1 MB host→browser limit.
        if payload.len() > 1_048_576 {
            let truncated = SaveResponse::error("io_error", "response exceeds 1 MB limit");
            write_message(writer, &serde_json::to_vec(&truncated)?)?;
        } else {
            write_message(writer, &payload)?;
        }
    }

    Ok(())
}

#[derive(Deserialize)]
struct ActionPeek {
    action: String,
}

#[cfg(test)]
fn dispatch(raw: &[u8]) -> SaveResponse {
    NativeSession::default().dispatch(raw)
}

#[derive(Default)]
struct NativeSession {
    video_import: VideoImportSession,
}

impl NativeSession {
    fn dispatch(&mut self, raw: &[u8]) -> SaveResponse {
        let action = match serde_json::from_slice::<ActionPeek>(raw) {
            Err(e) => {
                self.video_import.abort_active();
                return SaveResponse::error("invalid_request", &e.to_string());
            }
            Ok(p) => p.action,
        };

        let action_can_continue_active_upload = matches!(
            action.as_str(),
            "video_import_begin"
                | "video_import_chunk"
                | "video_import_finish"
                | "video_import_abort"
        );
        if self.video_import.is_active() && !action_can_continue_active_upload {
            self.video_import.abort_active();
            return SaveResponse::error(
                "invalid_request",
                "a browser video upload is active on this connection",
            );
        }

        match action.as_str() {
            "video_import_begin" => match serde_json::from_slice::<VideoImportBeginRequest>(raw) {
                Ok(request) => self.video_import.begin(request),
                Err(error) => {
                    self.video_import.abort_active();
                    SaveResponse::error("invalid_request", &error.to_string())
                }
            },
            "video_import_chunk" => match serde_json::from_slice::<VideoImportChunkRequest>(raw) {
                Ok(request) => self.video_import.chunk(request),
                Err(error) => {
                    self.video_import.abort_active();
                    SaveResponse::error("invalid_request", &error.to_string())
                }
            },
            "video_import_finish" => match serde_json::from_slice::<VideoImportEndRequest>(raw) {
                Ok(request) => self.video_import.finish(request),
                Err(error) => {
                    self.video_import.abort_active();
                    SaveResponse::error("invalid_request", &error.to_string())
                }
            },
            "video_import_abort" => match serde_json::from_slice::<VideoImportEndRequest>(raw) {
                Ok(request) => self.video_import.abort(request),
                Err(error) => {
                    self.video_import.abort_active();
                    SaveResponse::error("invalid_request", &error.to_string())
                }
            },
            "check" => match serde_json::from_slice::<CheckRequest>(raw) {
                Err(e) => SaveResponse::error("invalid_request", &e.to_string()),
                Ok(req) => match check::handle(req) {
                    Ok(resp) => resp,
                    Err(e) => SaveResponse::error("io_error", &e.to_string()),
                },
            },
            "save_selection" => match serde_json::from_slice::<SaveSelectionRequest>(raw) {
                Err(error) => SaveResponse::error("invalid_request", &error.to_string()),
                Ok(request) => match save_selection::handle(request) {
                    Ok(response) => response,
                    Err(error) => SaveResponse::error("io_error", &error.to_string()),
                },
            },
            "save_link" => match serde_json::from_slice::<SaveLinkRequest>(raw) {
                Err(error) => SaveResponse::error("invalid_request", &error.to_string()),
                Ok(request) => match save_link::handle(request) {
                    Ok(response) => response,
                    Err(error) => SaveResponse::error("io_error", &error.to_string()),
                },
            },
            _ => match serde_json::from_slice::<SaveRequest>(raw) {
                Err(e) => SaveResponse::error("invalid_request", &e.to_string()),
                Ok(req) => match save::handle(req) {
                    Ok(resp) => resp,
                    Err(e) => {
                        let (code, msg) = save::classify_error(&e);
                        SaveResponse::error(code, &msg)
                    }
                },
            },
        }
    }
}

/// Read one length-prefixed message from the native messaging stream.
/// Returns `UnexpectedEof` when the browser closes the pipe.
fn read_message(r: &mut impl Read) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Write one length-prefixed message to the native messaging stream.
fn write_message(w: &mut impl Write, msg: &[u8]) -> Result<()> {
    let len = (msg.len() as u32).to_le_bytes();
    w.write_all(&len)?;
    w.write_all(msg)?;
    w.flush()?;
    Ok(())
}

fn is_eof_error(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::UnexpectedEof)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed(json: &str) -> Vec<u8> {
        let bytes = json.as_bytes();
        let mut buf = (bytes.len() as u32).to_le_bytes().to_vec();
        buf.extend_from_slice(bytes);
        buf
    }

    #[test]
    fn round_trip_framing() {
        let payload = r#"{"hello":"world"}"#;
        let mut cursor = Cursor::new(framed(payload));
        let out = read_message(&mut cursor).unwrap();
        assert_eq!(out, payload.as_bytes());
    }

    #[test]
    fn invalid_request_returns_error_response() {
        let resp = dispatch(b"not json at all");
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("invalid_request"));
    }

    #[test]
    fn wrong_protocol_version_returns_error() {
        let msg = serde_json::json!({
            "protocol_version": 99,
            "action": "save",
            "metadata": {
                "url": "https://example.com",
                "canonical_url": "https://example.com",
                "title": "Test",
                "saved_at": "2026-06-13T15:00:00Z"
            },
            "markdown": "# Test",
            "images": []
        });
        let resp = dispatch(serde_json::to_vec(&msg).unwrap().as_slice());
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("invalid_request"));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use base64::Engine;
    use serde_json::Value;
    use std::{io::Cursor, sync::Mutex};
    use tempfile::TempDir;

    // Serialize all tests that touch CUTTINGS_LIBRARY to avoid races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn save_message(url: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "protocol_version": 4,
            "action": "save",
            "metadata": {
                "url": url,
                "canonical_url": url,
                "title": "Test Article",
                "theme_color": "#123456",
                "saved_at": "2026-06-13T15:00:00Z"
            },
            "markdown": "# Test Article\n\nSome content here.\n",
            "images": []
        }))
        .unwrap()
    }

    fn kind_save_message(
        kind: &str,
        source_url: &str,
        media_url: Option<&str>,
        markdown: &str,
    ) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "protocol_version": 4,
            "action": "save",
            "metadata": {
                "kind": kind,
                "url": source_url,
                "media_url": media_url,
                "canonical_url": source_url,
                "title": "Saved item",
                "site": "example.com",
                "excerpt": markdown,
                "saved_at": "2026-06-13T15:00:00Z"
            },
            "markdown": markdown,
            "images": []
        }))
        .unwrap()
    }

    fn save_link_message(url: &str) -> Vec<u8> {
        let social_url = "https://cdn.example.com/social.png";
        let favicon_url = "https://example.com/favicon.ico";
        serde_json::to_vec(&serde_json::json!({
            "protocol_version": 4,
            "action": "save_link",
            "metadata": {
                "kind": "article",
                "url": url,
                "canonical_url": url,
                "title": "Link title",
                "site": "Example",
                "theme_color": "#654321",
                "excerpt": "Link description",
                "saved_at": "2026-06-13T15:00:00Z"
            },
            "images": [
                {
                    "url": social_url,
                    "content_type": "image/png",
                    "data_base64": base64::engine::general_purpose::STANDARD.encode(b"social image")
                },
                {
                    "url": favicon_url,
                    "content_type": "image/x-icon",
                    "data_base64": base64::engine::general_purpose::STANDARD.encode(b"favicon")
                }
            ],
            "preview_url": social_url,
            "favicon_url": favicon_url
        }))
        .unwrap()
    }

    fn with_library<F, R>(f: F) -> R
    where
        F: FnOnce(&TempDir) -> R,
    {
        // Recover from a poisoned lock so a panic in one test doesn't cascade
        // into PoisonError failures in the others.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        std::env::set_var("CUTTINGS_LIBRARY", dir.path());
        let result = f(&dir);
        std::env::remove_var("CUTTINGS_LIBRARY");
        result
    }

    fn run_framed_messages(messages: &[Value]) -> Vec<Value> {
        let input = messages
            .iter()
            .fold(Vec::new(), |mut framed_input, message| {
                let payload = serde_json::to_vec(message).unwrap();
                framed_input.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                framed_input.extend_from_slice(&payload);
                framed_input
            });
        let mut reader = Cursor::new(input);
        let mut output = Vec::new();
        run_loop_with(&mut reader, &mut output).unwrap();

        let mut responses = Vec::new();
        let mut cursor = Cursor::new(output);
        loop {
            match read_message(&mut cursor) {
                Ok(payload) => responses.push(serde_json::from_slice(&payload).unwrap()),
                Err(error) if is_eof_error(&error) => break,
                Err(error) => panic!("invalid framed response: {error}"),
            }
        }
        responses
    }

    fn video_begin(upload_id: &str, url: &str, expected_bytes: Option<usize>) -> Value {
        serde_json::json!({
            "protocol_version": 4,
            "action": "video_import_begin",
            "upload_id": upload_id,
            "metadata": {
                "kind": "video",
                "url": url,
                "canonical_url": url,
                "title": "Framed browser video",
                "saved_at": "2026-08-25T12:00:00.000Z"
            },
            "content_type": "video/mp4",
            "expected_bytes": expected_bytes
        })
    }

    fn video_chunk(upload_id: &str, sequence: u64, bytes: &[u8]) -> Value {
        serde_json::json!({
            "protocol_version": 4,
            "action": "video_import_chunk",
            "upload_id": upload_id,
            "sequence": sequence,
            "data_base64": base64::engine::general_purpose::STANDARD.encode(bytes)
        })
    }

    fn video_end(action: &str, upload_id: &str) -> Value {
        serde_json::json!({
            "protocol_version": 4,
            "action": action,
            "upload_id": upload_id
        })
    }

    fn mp4_video_bytes(payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32 + payload.len());
        bytes.extend_from_slice(&24_u32.to_be_bytes());
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"isom");
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(b"isom");
        bytes.extend_from_slice(b"mp42");
        bytes.extend_from_slice(&u32::try_from(8 + payload.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(b"mdat");
        bytes.extend_from_slice(payload);
        bytes
    }

    fn staging_is_empty(library: &TempDir) -> bool {
        let staging = library.path().join(".cuttings-imports");
        !staging.is_dir() || std::fs::read_dir(staging).unwrap().next().is_none()
    }

    #[test]
    fn framed_video_import_persists_the_exact_local_video() {
        with_library(|dir| {
            let expected = mp4_video_bytes(b"first framed video chunk and its second chunk");
            let (first, second) = expected.split_at(16);
            let messages = vec![
                serde_json::json!({
                    "protocol_version": 4,
                    "action": "video_import_begin",
                    "upload_id": "upload-exact",
                    "metadata": {
                        "kind": "video",
                        "url": "https://example.com/clips/exact",
                        "canonical_url": "https://example.com/canonical/exact#player",
                        "title": "Exact browser video",
                        "author": "Example Director",
                        "site": "Example Cinema",
                        "theme_color": "#abcdef",
                        "lang": "en-IE",
                        "excerpt": "A framed browser video.",
                        "word_count": 4,
                        "saved_at": "2026-08-25T12:00:00.000Z"
                    },
                    "content_type": "video/mp4",
                    "expected_bytes": expected.len()
                }),
                serde_json::json!({
                    "protocol_version": 4,
                    "action": "video_import_chunk",
                    "upload_id": "upload-exact",
                    "sequence": 0,
                    "data_base64": base64::engine::general_purpose::STANDARD.encode(first)
                }),
                serde_json::json!({
                    "protocol_version": 4,
                    "action": "video_import_chunk",
                    "upload_id": "upload-exact",
                    "sequence": 1,
                    "data_base64": base64::engine::general_purpose::STANDARD.encode(second)
                }),
                serde_json::json!({
                    "protocol_version": 4,
                    "action": "video_import_finish",
                    "upload_id": "upload-exact"
                }),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses.len(), 4);
            assert!(responses.iter().all(|response| response["ok"] == true));
            let ack = serde_json::json!({ "protocol_version": 4, "ok": true });
            assert_eq!(responses[0], ack, "begin is a small ack");
            assert_eq!(responses[1], ack, "chunk is a small ack");
            assert_eq!(responses[2], ack, "chunk is a small ack");

            let id = responses[3]["id"].as_str().unwrap();
            let article_path = dir.path().join(responses[3]["path"].as_str().unwrap());
            let reading =
                oia_core::parse_reading(&std::fs::read_to_string(article_path).unwrap()).unwrap();
            assert_eq!(reading.metadata.kind, oia_core::ReadingKind::Video);
            assert_eq!(
                reading.metadata.canonical_url,
                "https://example.com/canonical/exact"
            );
            assert_eq!(reading.metadata.author.as_deref(), Some("Example Director"));
            assert_eq!(reading.metadata.site.as_deref(), Some("Example Cinema"));
            assert_eq!(reading.metadata.theme_color.as_deref(), Some("#abcdef"));
            assert_eq!(reading.metadata.lang.as_deref(), Some("en-IE"));
            assert_eq!(
                reading.metadata.excerpt.as_deref(),
                Some("A framed browser video.")
            );
            assert_eq!(reading.metadata.word_count, Some(4));
            let local_reference = reading
                .metadata
                .media_url
                .as_deref()
                .unwrap()
                .strip_prefix("cuttings-asset:")
                .unwrap();
            assert_eq!(
                std::fs::read(
                    dir.path()
                        .join("articles")
                        .join(&id[..2])
                        .join(id)
                        .join(local_reference)
                )
                .unwrap(),
                expected
            );
        });
    }

    #[test]
    fn framed_save_check_and_link_actions_remain_compatible() {
        with_library(|_dir| {
            let saved_url = "https://example.com/framed-existing-actions";
            let link_url = "https://example.com/framed-existing-link";
            let messages = vec![
                serde_json::from_slice(&save_message(saved_url)).unwrap(),
                serde_json::json!({
                    "protocol_version": 4,
                    "action": "check",
                    "url": saved_url
                }),
                serde_json::from_slice(&save_link_message(link_url)).unwrap(),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses.len(), 3);
            assert_eq!(responses[0]["ok"], true);
            assert_eq!(responses[1]["ok"], true);
            assert_eq!(responses[1]["saved"], true);
            assert_eq!(responses[2]["ok"], true);
        });
    }

    #[test]
    fn out_of_order_video_chunk_cancels_the_transfer() {
        with_library(|dir| {
            let recovery = mp4_video_bytes(b"order recovery");
            let messages = vec![
                video_begin(
                    "upload-order",
                    "https://example.com/clips/out-of-order",
                    Some(1),
                ),
                video_chunk("upload-order", 1, b"x"),
                video_begin(
                    "upload-recovery",
                    "https://example.com/clips/order-recovery",
                    Some(recovery.len()),
                ),
                video_chunk("upload-recovery", 0, &recovery),
                video_end("video_import_finish", "upload-recovery"),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses[1]["ok"], false);
            assert_eq!(responses[1]["error"], "invalid_request");
            assert_eq!(responses[2]["ok"], true, "the failed transfer was cleared");
            assert_eq!(responses[4]["ok"], true);
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn wrong_video_upload_id_cancels_the_transfer() {
        with_library(|dir| {
            let recovery = mp4_video_bytes(b"wrong id recovery");
            let messages = vec![
                video_begin(
                    "upload-right",
                    "https://example.com/clips/wrong-id",
                    Some(1),
                ),
                video_chunk("upload-wrong", 0, b"x"),
                video_begin(
                    "upload-after-wrong-id",
                    "https://example.com/clips/wrong-id-recovery",
                    Some(recovery.len()),
                ),
                video_chunk("upload-after-wrong-id", 0, &recovery),
                video_end("video_import_finish", "upload-after-wrong-id"),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses[1]["ok"], false);
            assert_eq!(responses[1]["error"], "invalid_request");
            assert_eq!(responses[2]["ok"], true, "the failed transfer was cleared");
            assert_eq!(responses[4]["ok"], true);
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn oversized_decoded_video_chunk_cancels_the_transfer() {
        with_library(|dir| {
            let oversized = vec![0xA5; 256 * 1024 + 1];
            let recovery = mp4_video_bytes(b"oversized chunk recovery");
            let messages = vec![
                video_begin(
                    "upload-oversized",
                    "https://example.com/clips/oversized",
                    Some(oversized.len()),
                ),
                video_chunk("upload-oversized", 0, &oversized),
                video_begin(
                    "upload-after-oversized",
                    "https://example.com/clips/oversized-recovery",
                    Some(recovery.len()),
                ),
                video_chunk("upload-after-oversized", 0, &recovery),
                video_end("video_import_finish", "upload-after-oversized"),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses[1]["ok"], false);
            assert!(responses[1]["message"]
                .as_str()
                .unwrap()
                .contains("262144 byte limit"));
            assert_eq!(responses[2]["ok"], true, "the failed transfer was cleared");
            assert_eq!(responses[4]["ok"], true);
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn declared_video_size_mismatch_cancels_the_transfer() {
        with_library(|dir| {
            let recovery = mp4_video_bytes(b"size mismatch recovery");
            let messages = vec![
                video_begin(
                    "upload-mismatch",
                    "https://example.com/clips/mismatch",
                    Some(2),
                ),
                video_chunk("upload-mismatch", 0, b"x"),
                video_end("video_import_finish", "upload-mismatch"),
                video_begin(
                    "upload-after-mismatch",
                    "https://example.com/clips/mismatch-recovery",
                    Some(recovery.len()),
                ),
                video_chunk("upload-after-mismatch", 0, &recovery),
                video_end("video_import_finish", "upload-after-mismatch"),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses[2]["ok"], false);
            assert!(responses[2]["message"]
                .as_str()
                .unwrap()
                .contains("did not match the declared size"));
            assert_eq!(responses[3]["ok"], true, "the failed transfer was cleared");
            assert_eq!(responses[5]["ok"], true);
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn abort_cleans_the_partial_video_and_allows_another_upload() {
        with_library(|dir| {
            let recovery = mp4_video_bytes(b"abort recovery");
            let messages = vec![
                video_begin("upload-abort", "https://example.com/clips/abort", None),
                video_chunk("upload-abort", 0, b"partial"),
                video_end("video_import_abort", "upload-abort"),
                video_begin(
                    "upload-after-abort",
                    "https://example.com/clips/abort-recovery",
                    Some(recovery.len()),
                ),
                video_chunk("upload-after-abort", 0, &recovery),
                video_end("video_import_finish", "upload-after-abort"),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(
                responses[2],
                serde_json::json!({
                    "protocol_version": 4,
                    "ok": true
                })
            );
            assert_eq!(responses[3]["ok"], true);
            assert_eq!(responses[5]["ok"], true);
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn connection_eof_cleans_an_unfinished_video_upload() {
        with_library(|dir| {
            let responses = run_framed_messages(&[
                video_begin("upload-eof", "https://example.com/clips/eof", None),
                video_chunk("upload-eof", 0, b"partial bytes before EOF"),
            ]);

            assert_eq!(responses.len(), 2);
            assert!(responses.iter().all(|response| response["ok"] == true));
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn second_begin_rejects_and_cleans_the_active_video_upload() {
        with_library(|dir| {
            let recovery = mp4_video_bytes(b"second begin recovery");
            let messages = vec![
                video_begin(
                    "upload-first",
                    "https://example.com/clips/first-active",
                    None,
                ),
                video_chunk("upload-first", 0, b"partial"),
                video_begin(
                    "upload-second",
                    "https://example.com/clips/second-active",
                    Some(1),
                ),
                video_begin(
                    "upload-third",
                    "https://example.com/clips/third-active",
                    Some(recovery.len()),
                ),
                video_chunk("upload-third", 0, &recovery),
                video_end("video_import_finish", "upload-third"),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses[2]["ok"], false);
            assert!(responses[2]["message"]
                .as_str()
                .unwrap()
                .contains("already active"));
            assert_eq!(
                responses[3]["ok"], true,
                "the rejected begin cleared the transfer"
            );
            assert_eq!(responses[5]["ok"], true);
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn unknown_video_action_cancels_the_transfer() {
        with_library(|dir| {
            let recovery = mp4_video_bytes(b"unknown action recovery");
            let messages = vec![
                video_begin(
                    "upload-unknown-action",
                    "https://example.com/clips/unknown-action",
                    None,
                ),
                video_chunk("upload-unknown-action", 0, b"partial"),
                serde_json::json!({
                    "protocol_version": 4,
                    "action": "video_import_unknown",
                    "upload_id": "upload-unknown-action"
                }),
                video_begin(
                    "upload-after-unknown-action",
                    "https://example.com/clips/unknown-action-recovery",
                    Some(recovery.len()),
                ),
                video_chunk("upload-after-unknown-action", 0, &recovery),
                video_end("video_import_finish", "upload-after-unknown-action"),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses[2]["ok"], false);
            assert_eq!(responses[3]["ok"], true, "the failed transfer was cleared");
            assert_eq!(responses[5]["ok"], true);
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn video_import_begin_requires_video_metadata() {
        with_library(|dir| {
            let mut begin = video_begin(
                "upload-article-kind",
                "https://example.com/clips/article-kind",
                Some(1),
            );
            begin["metadata"]["kind"] = serde_json::json!("article");

            let responses = run_framed_messages(&[begin]);
            assert_eq!(responses[0]["ok"], false);
            assert_eq!(responses[0]["error"], "invalid_request");
            assert!(responses[0]["message"]
                .as_str()
                .unwrap()
                .contains("video metadata"));
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn zero_byte_video_chunk_cancels_the_transfer() {
        with_library(|dir| {
            let recovery = mp4_video_bytes(b"empty chunk recovery");
            let messages = vec![
                video_begin(
                    "upload-empty-chunk",
                    "https://example.com/clips/empty-chunk",
                    None,
                ),
                video_chunk("upload-empty-chunk", 0, b""),
                video_begin(
                    "upload-after-empty-chunk",
                    "https://example.com/clips/empty-chunk-recovery",
                    Some(recovery.len()),
                ),
                video_chunk("upload-after-empty-chunk", 0, &recovery),
                video_end("video_import_finish", "upload-after-empty-chunk"),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses[1]["ok"], false);
            assert!(responses[1]["message"]
                .as_str()
                .unwrap()
                .contains("must not be empty"));
            assert_eq!(responses[2]["ok"], true, "the failed transfer was cleared");
            assert_eq!(responses[4]["ok"], true);
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn duplicate_browser_video_returns_the_duplicate_protocol_error() {
        with_library(|dir| {
            let url = "https://example.com/clips/duplicate-browser-video";
            let video = mp4_video_bytes(b"duplicate browser video");
            let messages = vec![
                video_begin("upload-duplicate-first", url, Some(video.len())),
                video_chunk("upload-duplicate-first", 0, &video),
                video_end("video_import_finish", "upload-duplicate-first"),
                video_begin("upload-duplicate-second", url, Some(video.len())),
                video_chunk("upload-duplicate-second", 0, &video),
                video_end("video_import_finish", "upload-duplicate-second"),
            ];

            let responses = run_framed_messages(&messages);
            assert_eq!(responses[2]["ok"], true);
            assert_eq!(responses[5]["ok"], false);
            assert_eq!(responses[5]["error"], "duplicate");
            assert!(responses[5]["message"]
                .as_str()
                .unwrap()
                .contains("already exists"));
            assert!(staging_is_empty(dir));
        });
    }

    #[test]
    fn save_writes_article_to_disk() {
        with_library(|dir| {
            let resp = dispatch(&save_message("https://example.com/article-1"));
            assert!(resp.ok, "expected ok, got error: {:?}", resp.error);

            // Locate the file via the fan-out path the response reports.
            let rel = resp.path.as_deref().unwrap();
            let article_path = dir.path().join(rel);
            assert!(
                article_path.exists(),
                "article file was not written to disk"
            );

            let content = std::fs::read_to_string(&article_path).unwrap();
            let reading = oia_core::parse_reading(&content).unwrap();
            assert_eq!(reading.metadata.theme_color.as_deref(), Some("#123456"));
            assert!(
                content.contains("format_version: 1"),
                "missing format_version in frontmatter"
            );
            assert!(content.contains("Test Article"), "missing title in file");
            assert!(
                content.contains("Some content here."),
                "missing body in file"
            );
        });
    }

    #[test]
    fn protocol_four_selection_dispatch_returns_kind_and_persists_article_highlight() {
        with_library(|dir| {
            let url = "https://example.com/selection-dispatch";
            let mut message: Value = serde_json::from_slice(&save_message(url)).unwrap();
            message["action"] = serde_json::json!("save_selection");
            message["selected_text"] = serde_json::json!("Some content here.");
            let responses = run_framed_messages(&[message]);
            assert_eq!(responses[0]["protocol_version"], 4);
            assert_eq!(responses[0]["ok"], true);
            assert_eq!(responses[0]["selection_kind"], "highlight");
            let id = responses[0]["id"].as_str().unwrap();
            let root = oia_core::LibraryRoot::new(dir.path()).unwrap();
            let highlights = oia_core::list_highlights(&root, id).unwrap();
            assert_eq!(highlights[0].text, "Some content here.");
            assert_eq!(oia_core::scan_library(&root).unwrap().len(), 1);
        });
    }

    #[test]
    fn success_response_shape() {
        with_library(|_dir| {
            let resp = dispatch(&save_message("https://example.com/resp-shape"));
            assert!(resp.ok);
            assert_eq!(resp.protocol_version, 4);
            assert!(serde_json::to_value(&resp)
                .unwrap()
                .get("selection_kind")
                .is_none());

            let id = resp
                .id
                .as_deref()
                .expect("id missing from success response");
            assert_eq!(id.len(), 64, "content-addressed id is 64-char SHA-256 hex");

            let path = resp
                .path
                .as_deref()
                .expect("path missing from success response");
            // Per-reading folder: articles/<first 2 hex chars>/<id>/article.md
            assert_eq!(path, format!("articles/{}/{id}/article.md", &id[..2]));
        });
    }

    #[test]
    fn duplicate_url_returns_duplicate_error() {
        with_library(|_dir| {
            let msg = save_message("https://example.com/dup");
            dispatch(&msg);
            let resp = dispatch(&msg);
            assert!(!resp.ok, "second save with same canonical_url should fail");
            assert_eq!(resp.error.as_deref(), Some("duplicate"));
            assert!(
                resp.message
                    .as_deref()
                    .unwrap_or("")
                    .contains("already exists"),
                "duplicate message should mention the conflict"
            );
        });
    }

    #[test]
    fn save_link_writes_metadata_and_assets_then_full_capture_upgrades_it() {
        with_library(|dir| {
            let url = "https://example.com/to-upgrade";
            let link_response = dispatch(&save_link_message(url));
            assert!(link_response.ok, "link save should succeed");

            let link_content =
                std::fs::read_to_string(dir.path().join(link_response.path.as_deref().unwrap()))
                    .unwrap();
            let link = oia_core::parse_reading(&link_content).unwrap();
            assert!(link.metadata.lightweight);
            assert_eq!(link.metadata.title, "Link title");
            assert_eq!(link.metadata.excerpt.as_deref(), Some("Link description"));
            assert_eq!(link.metadata.theme_color.as_deref(), Some("#654321"));
            assert_eq!(link.body, format!("[Open link](<{url}>)\n"));
            let preview_asset = link.metadata.preview_asset.clone().unwrap();
            let favicon_asset = link.metadata.favicon_asset.clone().unwrap();
            assert_eq!(
                std::fs::read(
                    dir.path()
                        .join(link_response.path.as_deref().unwrap())
                        .parent()
                        .unwrap()
                        .join(&preview_asset)
                )
                .unwrap(),
                b"social image"
            );
            assert_eq!(
                std::fs::read(
                    dir.path()
                        .join(link_response.path.as_deref().unwrap())
                        .parent()
                        .unwrap()
                        .join(&favicon_asset)
                )
                .unwrap(),
                b"favicon"
            );

            let response = dispatch(&save_message(url));
            assert!(response.ok, "upgrade should be a protocol-v4 success");
            assert_eq!(response.protocol_version, 4);
            assert_eq!(response.id, link_response.id);
            assert_eq!(response.error, None);

            let reading = oia_core::parse_reading(
                &std::fs::read_to_string(dir.path().join(response.path.unwrap())).unwrap(),
            )
            .unwrap();
            assert!(!reading.metadata.lightweight);
            assert_eq!(reading.body, "# Test Article\n\nSome content here.\n");
            assert_eq!(reading.metadata.preview_asset, Some(preview_asset));
            assert_eq!(reading.metadata.favicon_asset, Some(favicon_asset));
        });
    }

    #[test]
    fn no_library_returns_library_not_configured() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        // Isolate BOTH library sources: the env var and the
        // `$HOME/.config/cuttings/library` fallback. Pointing HOME at an
        // empty temp dir ensures the host can't resolve a real library that a
        // developer machine happens to have configured.
        let home = TempDir::new().unwrap();
        let prev_home = std::env::var_os("HOME");
        std::env::set_var("HOME", home.path());
        std::env::remove_var("CUTTINGS_LIBRARY");

        let resp = dispatch(&save_message("https://example.com/no-lib"));

        // Restore HOME before asserting so an assertion failure can't leak the
        // overridden value into other tests.
        match prev_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }

        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("library_not_configured"));
    }

    #[test]
    fn supplied_image_is_written_and_unsupplied_stays_remote() {
        with_library(|dir| {
            // One image comes with bytes; the other has none supplied.
            let bytes = b"\x89PNG\r\n\x1a\npixels";
            let data_b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            let msg = serde_json::to_vec(&serde_json::json!({
                "protocol_version": 4,
                "action": "save",
                "metadata": {
                    "url": "https://example.com/img-test",
                    "canonical_url": "https://example.com/img-test",
                    "title": "Image Test",
                    "saved_at": "2026-06-13T15:00:00Z"
                },
                "markdown": "# Image Test\n\n![Got](https://cdn.example.com/got.png)\n![Missing](https://cdn.example.com/missing.png)\n",
                "images": [
                    {
                        "url": "https://cdn.example.com/got.png",
                        "content_type": "image/png",
                        "data_base64": data_b64
                    }
                ]
            }))
            .unwrap();

            let resp = dispatch(&msg);
            assert!(resp.ok, "save should succeed: {:?}", resp.error);

            let content =
                std::fs::read_to_string(dir.path().join(resp.path.as_deref().unwrap())).unwrap();
            let id = resp.id.as_deref().unwrap();
            // The supplied image is rewritten to a local asset, linked relative
            // to the article file that sits beside its assets/ folder.
            assert!(
                content.contains("![Got](assets/"),
                "supplied image should be rewritten to a local path:\n{content}"
            );
            assert!(
                !content.contains("cdn.example.com/got.png"),
                "supplied image's remote URL should be gone"
            );
            // ...and its bytes are on disk under the reading's own assets/ folder.
            let asset = dir
                .path()
                .join("articles")
                .join(&id[..2])
                .join(id)
                .join("assets")
                .join(format!("{}.png", oia_core::sha256_hex(bytes)));
            assert_eq!(std::fs::read(&asset).unwrap(), bytes);
            let reading = oia_core::parse_reading(&content).unwrap();
            assert_eq!(
                reading.metadata.preview_asset,
                Some(format!("assets/{}.png", oia_core::sha256_hex(bytes)))
            );
            // The unsupplied image keeps its remote URL as a placeholder.
            assert!(
                content.contains("https://cdn.example.com/missing.png"),
                "an image with no supplied bytes keeps its remote URL"
            );
        });
    }

    #[test]
    fn media_identity_distinguishes_items_on_one_source_page() {
        with_library(|dir| {
            let source = "https://example.com/gallery";
            let first = kind_save_message(
                "image",
                source,
                Some("https://cdn.example.com/first.jpg"),
                "![First](https://cdn.example.com/first.jpg)",
            );
            let second = kind_save_message(
                "image",
                source,
                Some("https://cdn.example.com/second.jpg"),
                "![Second](https://cdn.example.com/second.jpg)",
            );

            let first_response = dispatch(&first);
            let second_response = dispatch(&second);
            assert!(first_response.ok);
            assert!(second_response.ok);
            assert_ne!(first_response.id, second_response.id);

            let duplicate = dispatch(&first);
            assert_eq!(duplicate.error.as_deref(), Some("duplicate"));

            let first_path = dir.path().join(first_response.path.unwrap());
            let reading =
                oia_core::parse_reading(&std::fs::read_to_string(first_path).unwrap()).unwrap();
            assert_eq!(reading.metadata.kind, oia_core::ReadingKind::Image);
            assert_eq!(reading.metadata.url, source);
            assert_eq!(reading.metadata.canonical_url, source);
            assert_eq!(reading.metadata.site.as_deref(), Some("example.com"));
            assert_eq!(
                reading.metadata.media_url.as_deref(),
                Some("https://cdn.example.com/first.jpg")
            );
        });
    }

    #[test]
    fn media_save_requires_media_url() {
        with_library(|_dir| {
            let response = dispatch(&kind_save_message(
                "video",
                "https://example.com/watch",
                None,
                "A video",
            ));
            assert_eq!(response.error.as_deref(), Some("invalid_request"));
        });
    }

    #[test]
    fn ordinary_video_save_rejects_reference_only_media_urls() {
        with_library(|dir| {
            for (source, media_url) in [
                (
                    "https://example.com/watch/blob",
                    "blob:https://example.com/7e64a8cf",
                ),
                (
                    "https://example.com/watch/http",
                    "https://media.example.com/movie.mp4",
                ),
            ] {
                let response = dispatch(&kind_save_message(
                    "video",
                    source,
                    Some(media_url),
                    "A saved video",
                ));
                assert!(!response.ok, "ordinary video save must reject {media_url}");
                assert_eq!(response.error.as_deref(), Some("invalid_request"));
                assert!(response
                    .message
                    .as_deref()
                    .unwrap()
                    .contains("streaming video import"));
                assert_eq!(response.id, None);
                assert_eq!(response.path, None);
            }

            assert!(
                !dir.path().join("articles").exists(),
                "a rejected ordinary video save must not create a reading"
            );
        });
    }

    #[test]
    fn quote_identity_dedupes_normalized_selection_but_allows_another_quote() {
        with_library(|dir| {
            let source = "https://example.com/post";
            let first = dispatch(&kind_save_message(
                "quote",
                source,
                None,
                "A selected\npassage.",
            ));
            assert!(first.ok);

            let same_normalized = dispatch(&kind_save_message(
                "quote",
                source,
                None,
                "  A selected passage.  ",
            ));
            assert_eq!(same_normalized.error.as_deref(), Some("duplicate"));

            let different = dispatch(&kind_save_message(
                "quote",
                source,
                None,
                "A different passage.",
            ));
            assert!(different.ok);
            assert_ne!(first.id, different.id);

            let reading = oia_core::parse_reading(
                &std::fs::read_to_string(dir.path().join(first.path.unwrap())).unwrap(),
            )
            .unwrap();
            assert_eq!(reading.metadata.kind, oia_core::ReadingKind::Quote);
            assert_eq!(reading.metadata.url, source);
            assert_eq!(reading.metadata.media_url, None);
            assert_eq!(reading.body, "A selected\npassage.\n");
        });
    }

    #[test]
    fn check_returns_not_saved_for_empty_library() {
        // No articles saved yet: check scans an empty (or absent) articles/ dir
        // and reports saved: false.
        with_library(|_dir| {
            let msg = serde_json::to_vec(&serde_json::json!({
                "protocol_version": 4,
                "action": "check",
                "url": "https://example.com/not-there"
            }))
            .unwrap();
            let resp = dispatch(&msg);
            assert!(resp.ok);
            assert_eq!(resp.saved, Some(false));
        });
    }

    #[test]
    fn check_returns_not_saved_for_unknown_url() {
        with_library(|_dir| {
            // Save one article, then check a different URL.
            dispatch(&save_message("https://example.com/saved"));

            let msg = serde_json::to_vec(&serde_json::json!({
                "protocol_version": 4,
                "action": "check",
                "url": "https://example.com/not-there"
            }))
            .unwrap();
            let resp = dispatch(&msg);
            assert!(resp.ok);
            assert_eq!(resp.saved, Some(false));
        });
    }

    #[test]
    fn check_returns_saved_after_save() {
        with_library(|_dir| {
            // Saving writes the article file; check scans articles/ directly, so
            // it sees the save with no index or app involvement.
            dispatch(&save_message("https://example.com/for-check"));

            let msg = serde_json::to_vec(&serde_json::json!({
                "protocol_version": 4,
                "action": "check",
                "url": "https://example.com/for-check"
            }))
            .unwrap();
            let resp = dispatch(&msg);
            assert!(resp.ok);
            assert_eq!(resp.saved, Some(true));
            assert!(resp.id.is_some());
        });
    }

    #[test]
    fn missing_required_field_returns_invalid_request() {
        // canonical_url, title, and saved_at are all absent — serde must reject this.
        let msg = serde_json::to_vec(&serde_json::json!({
            "protocol_version": 4,
            "action": "save",
            "metadata": { "url": "https://example.com" },
            "markdown": "# Test",
            "images": []
        }))
        .unwrap();

        let resp = dispatch(&msg);
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("invalid_request"));
    }
}
