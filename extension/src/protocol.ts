// SPDX-License-Identifier: MIT

/** The kind of item being saved. */
export type SaveKind = "article" | "image" | "video" | "quote";

/** Metadata sent alongside the Markdown body in a save request. */
export interface SaveRequestMetadata {
  kind: SaveKind;
  url: string;
  canonical_url: string;
  /**
   * A direct standalone image URL or legacy video identity. Current browser
   * video imports omit this field and stream bytes separately; the host writes
   * their local asset reference after the upload finishes. Article and quote
   * saves omit it.
   */
  media_url?: string;
  title: string;
  author?: string;
  site?: string;
  /** First page-declared theme colour; core validates and normalizes it for storage. */
  theme_color?: string;
  /** Raw live structured product evidence, bounded to 512 KiB; interpreted by core. */
  product_html?: string;
  lang?: string;
  excerpt?: string;
  word_count?: number;
  saved_at: string;
}

/** The wire protocol version shared with the native host. Version 4 adds the
 * bounded, backpressured video-import actions below. */
export const PROTOCOL_VERSION = 4;

/**
 * One image the extension captured from the page (reusing the browser's cache),
 * base64-encoded so it can ride inside the JSON save message. The host decodes
 * and writes these; it never downloads anything itself.
 */
export interface ImageData {
  /** Source lookup key: a Markdown URL or an explicit preview/favicon role URL. */
  url: string;
  /** The response's `Content-Type`, used by the host to pick a file extension. */
  content_type: string;
  /** Standard base64 of the raw image bytes. */
  data_base64: string;
}

/** Message sent from the extension to the native host. */
export interface SaveRequest {
  protocol_version: typeof PROTOCOL_VERSION;
  action: "save";
  metadata: SaveRequestMetadata;
  markdown: string;
  images: ImageData[];
  /** Source URL whose captured bytes should become metadata.preview_asset. */
  preview_url?: string;
  /** Source URL whose captured bytes should become metadata.favicon_asset. */
  favicon_url?: string;
}

/** Serialized DOM facts captured for the exact context-menu text selection. */
export interface SelectionContext {
  version: 1;
  selected_text: string;
  enclosing_element?: "blockquote" | "q";
}

/** Save selected text; core decides whether it is a highlight or quotation. */
export interface SaveSelectionRequest extends Omit<SaveRequest, "action"> {
  action: "save_selection";
  selected_text: string;
  selection_context?: SelectionContext;
}

/**
 * Save an HTTP(S) URL without extracting cleaned article content. Live page
 * metadata and preview/favicon bytes may still be captured; the native host
 * routes them through the core's lightweight-link path so a later article
 * capture can upgrade the same URL-derived reading in place.
 */
export interface SaveLinkRequest {
  protocol_version: typeof PROTOCOL_VERSION;
  action: "save_link";
  metadata: SaveRequestMetadata;
  images: ImageData[];
  preview_url?: string;
  favicon_url?: string;
}

/** Metadata retained for a browser-captured local video import. The selected
 * source URL is deliberately absent; the host writes a local media_url when
 * the upload finishes. */
export type VideoImportMetadata = Omit<SaveRequestMetadata, "kind" | "media_url"> & {
  kind: "video";
  media_url?: never;
};

export interface VideoImportBeginRequest {
  protocol_version: typeof PROTOCOL_VERSION;
  action: "video_import_begin";
  upload_id: string;
  metadata: VideoImportMetadata;
  content_type: string;
  expected_bytes?: number;
}

export interface VideoImportChunkRequest {
  protocol_version: typeof PROTOCOL_VERSION;
  action: "video_import_chunk";
  upload_id: string;
  /** Zero-based and strictly monotonic within one upload. */
  sequence: number;
  /** Standard base64; decoded bytes are capped at 256 KiB per message. */
  data_base64: string;
}

export interface VideoImportFinishRequest {
  protocol_version: typeof PROTOCOL_VERSION;
  action: "video_import_finish";
  upload_id: string;
}

export interface VideoImportAbortRequest {
  protocol_version: typeof PROTOCOL_VERSION;
  action: "video_import_abort";
  upload_id: string;
}

export type VideoImportRequest =
  | VideoImportBeginRequest
  | VideoImportChunkRequest
  | VideoImportFinishRequest
  | VideoImportAbortRequest;

export interface SaveResponseSuccess {
  protocol_version: typeof PROTOCOL_VERSION;
  ok: true;
  id: string;
  path: string;
  selection_kind?: "highlight" | "quote";
}

export interface SaveResponseError {
  protocol_version: typeof PROTOCOL_VERSION;
  ok: false;
  error: string;
  message: string;
}

export type SaveResponse = SaveResponseSuccess | SaveResponseError;

/** Begin/chunk/abort acknowledge without a saved-reading path. Finish returns
 * the existing SaveResponse success shape. */
export interface VideoImportAckSuccess {
  protocol_version: typeof PROTOCOL_VERSION;
  ok: true;
}

export type VideoImportResponse = VideoImportAckSuccess | SaveResponse;

/** Message sent from the extension to check whether a URL is already saved. */
export interface CheckRequest {
  protocol_version: typeof PROTOCOL_VERSION;
  action: "check";
  url: string;
}

export interface CheckResponse {
  protocol_version: typeof PROTOCOL_VERSION;
  ok: true;
  saved: boolean;
  id?: string;
}
