// SPDX-License-Identifier: MIT

import { extractStandaloneMedia } from "./media.js";
import { fetchPageVideoSource } from "./page-video-source.js";
import {
  PROTOCOL_VERSION,
  type SaveRequestMetadata,
  type SaveResponse,
  type VideoImportAbortRequest,
  type VideoImportChunkRequest,
  type VideoImportMetadata,
  type VideoImportRequest,
  type VideoImportResponse,
} from "./protocol.js";
import { recordVideoElement as recordRenderedVideoElement } from "./video-element-recorder.js";
import {
  cancelVideoReader,
  openVideoResponse,
  PAGE_VIDEO_READY_TIMEOUT_MS,
  readVideoChunk,
  releaseVideoReader,
  VIDEO_PREPARATION_HEARTBEAT_MS,
  VIDEO_PREPARATION_TIMEOUT_MS,
  VIDEO_TRANSFER_FINISH_TIMEOUT_MS,
  VIDEO_TRANSFER_TIMEOUT_MS,
  VideoTransferTimeoutError,
  waitForVideoTransfer,
} from "./video-transfer.js";

export const VIDEO_IMPORT_PORT_NAME = "oia-video-import";
export const MAX_VIDEO_IMPORT_CHUNK_BYTES = 256 * 1024;

/** Document/worker control only; never sent to the native messaging host. */
export interface VideoImportPreparationMessage {
  protocol_version: typeof PROTOCOL_VERSION;
  action: "video_import_prepare";
  upload_id: string;
}

export type VideoImportPortMessage = VideoImportRequest | VideoImportPreparationMessage;

export class VideoImportError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "VideoImportError";
  }
}

interface ListenerEvent<T extends (...args: never[]) => void> {
  addListener(listener: T): void;
  removeListener(listener: T): void;
}

/** The small runtime.Port surface used by the document-side uploader. Keeping
 * it structural makes the streaming contract testable without a browser. */
export interface VideoImportPort {
  postMessage(message: VideoImportPortMessage): void;
  disconnect(): void;
  onMessage: ListenerEvent<(message: VideoImportResponse) => void>;
  onDisconnect: ListenerEvent<() => void>;
}

export interface VideoImportOptions {
  doc: Document;
  pageUrl: string;
  mediaUrl: string;
  fetchVideo?: (url: string) => Promise<Response>;
  fetchPageVideo?: (url: string) => Promise<Response>;
  recordVideoElement?: (doc: Document, url: string) => Promise<Response>;
  connect?: (name: string) => VideoImportPort;
  createUploadId?: () => string;
  savedAt?: string;
}

export interface VideoImportResult {
  metadata: VideoImportMetadata;
  response: SaveResponse;
}

/** Compatibility shape retained for the focused object-URL tests. Production
 * video captures use the protocol-agnostic `VideoImportOptions` path below. */
export interface BlobVideoImportOptions extends Omit<
  VideoImportOptions,
  "mediaUrl" | "fetchVideo" | "fetchPageVideo"
> {
  blobUrl: string;
  fetchBlob?: (url: string) => Promise<Response>;
  fetchPageBlob?: (url: string) => Promise<Response>;
}

export type BlobVideoImportResult = VideoImportResult;

interface OpenedDocumentVideo {
  reader: ReadableStreamDefaultReader<Uint8Array>;
  initial: { chunks: Uint8Array[]; signature: Uint8Array };
  contentType: string;
  expectedBytes?: number;
}

class VideoSourceUnavailableError extends Error {}

/**
 * Fetch or record a video inside its owning tab and relay the body to the
 * worker one bounded chunk at a time. Reading the next stream value waits for
 * the native acknowledgement of every chunk already derived from the current
 * value, keeping memory bounded and providing end-to-end backpressure.
 */
export async function importVideo({
  doc,
  pageUrl,
  mediaUrl,
  fetchVideo,
  fetchPageVideo,
  recordVideoElement,
  connect = (name) => chrome.runtime.connect({ name }) as VideoImportPort,
  createUploadId = () => crypto.randomUUID(),
  savedAt = new Date().toISOString(),
}: VideoImportOptions): Promise<VideoImportResult> {
  const uploadId = createUploadId();
  // connectNative in the relay keeps the awaiting worker alive while this
  // document prepares its source. Preparation heartbeats are scoped below.
  const client = new VideoImportPortClient(connect(VIDEO_IMPORT_PORT_NAME));
  let preparationTimer: ReturnType<typeof setInterval> | undefined = setInterval(
    () => client.preparing(uploadId),
    VIDEO_PREPARATION_HEARTBEAT_MS,
  );
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
  const preparation = openDocumentVideo(
    doc,
    mediaUrl,
    fetchVideo,
    fetchPageVideo,
    recordVideoElement,
    client.signal,
  );

  try {
    const opened = await waitForVideoTransfer(
      preparation,
      "The video source took too long to prepare.",
      VIDEO_PREPARATION_TIMEOUT_MS,
      client.signal,
      (error) => client.cancelSource(error),
    );
    clearInterval(preparationTimer);
    preparationTimer = undefined;
    reader = opened.reader;
    const extracted = extractStandaloneMedia(doc, pageUrl, "video", mediaUrl, savedAt);
    const metadata = importMetadata(extracted.metadata);

    await requireAck(
      client.request({
        protocol_version: PROTOCOL_VERSION,
        action: "video_import_begin",
        upload_id: uploadId,
        metadata,
        content_type: opened.contentType,
        ...(opened.expectedBytes === undefined ? {} : { expected_bytes: opened.expectedBytes }),
      }),
    );

    let sequence = 0;
    for (const bytes of opened.initial.chunks) {
      sequence = await sendBytes(client, uploadId, sequence, bytes);
    }
    while (true) {
      const next = await readVideoChunk(reader, client.signal);
      if (next.done) break;
      if (next.value?.byteLength) {
        sequence = await sendBytes(client, uploadId, sequence, next.value);
      }
    }

    const finish = await client.request({
      protocol_version: PROTOCOL_VERSION,
      action: "video_import_finish",
      upload_id: uploadId,
    });
    if (!finish.ok) {
      if (finish.error === "duplicate") return { metadata, response: finish };
      throw new VideoImportError(finish.error, finish.message || finish.error);
    }
    if (!("id" in finish) || !("path" in finish)) {
      throw new Error("The Óia app returned an incomplete video save response.");
    }

    return { metadata, response: finish };
  } catch (error) {
    clearInterval(preparationTimer);
    // A disconnect/timeout can win as preparation returns its owned reader.
    // Observe that late success and cancel it unless this upload adopted it.
    void preparation.then(
      (opened) => {
        if (opened.reader !== reader) void cancelVideoReader(opened.reader, error);
      },
      () => undefined,
    );
    client.abort(uploadId);
    client.close();
    if (reader) await cancelVideoReader(reader, error);
    throw error instanceof VideoTransferTimeoutError
      ? new VideoImportError("video_timeout", error.message)
      : error;
  } finally {
    clearInterval(preparationTimer);
    if (reader) releaseVideoReader(reader);
    client.close();
  }
}

export function importBlobVideo({
  blobUrl,
  fetchBlob,
  fetchPageBlob,
  ...options
}: BlobVideoImportOptions): Promise<BlobVideoImportResult> {
  return importVideo({
    ...options,
    mediaUrl: blobUrl,
    fetchVideo: fetchBlob,
    fetchPageVideo: fetchPageBlob,
  });
}

async function openDocumentVideo(
  doc: Document,
  mediaUrl: string,
  fetchVideo: ((url: string) => Promise<Response>) | undefined,
  fetchPageVideo: ((url: string) => Promise<Response>) | undefined,
  recordVideoElement: ((doc: Document, url: string) => Promise<Response>) | undefined,
  signal: AbortSignal,
): Promise<OpenedDocumentVideo> {
  try {
    return await probeVideoResponse(
      await openVideoResponse(
        (fetchSignal) =>
          fetchVideo ? fetchVideo(mediaUrl) : fetch(mediaUrl, { signal: fetchSignal }),
        signal,
      ),
      signal,
    );
  } catch (contentError) {
    signal.throwIfAborted();
    if (!isFallbackVideoSourceError(contentError)) throw contentError;
  }

  try {
    return await probeVideoResponse(
      await openVideoResponse(
        (pageSignal) =>
          fetchPageVideo
            ? fetchPageVideo(mediaUrl)
            : fetchPageVideoSource(mediaUrl, undefined, undefined, pageSignal),
        signal,
        VIDEO_TRANSFER_TIMEOUT_MS + PAGE_VIDEO_READY_TIMEOUT_MS,
      ),
      signal,
    );
  } catch {
    signal.throwIfAborted();
    // A MAIN-world stream can still fail for an inaccessible source, and some
    // WebExtension hosts may not support MAIN injection. The exact rendered
    // element in the isolated world is the final capability-based fallback.
  }

  return probeVideoResponse(
    await openVideoResponse(
      (recordingSignal) =>
        recordVideoElement
          ? recordVideoElement(doc, mediaUrl)
          : recordRenderedVideoElement(doc, mediaUrl, { signal: recordingSignal }),
      signal,
    ),
    signal,
  );
}

async function probeVideoResponse(
  response: Response,
  signal: AbortSignal,
): Promise<OpenedDocumentVideo> {
  if (!response.ok || !response.body) {
    throw new VideoSourceUnavailableError("The temporary video could not be read from this page.");
  }

  const reader = response.body.getReader();
  try {
    const declaredContentType = response.headers.get("Content-Type");
    const initial = await readInitialVideoChunks(reader, declaredContentType, signal);
    return {
      reader,
      initial,
      contentType: resolveVideoContentType(declaredContentType, initial.signature),
      expectedBytes: parseExpectedBytes(response.headers.get("Content-Length")),
    };
  } catch (error) {
    await cancelVideoReader(reader, error);
    throw error;
  }
}

function isFallbackVideoSourceError(error: unknown): boolean {
  return (
    error instanceof TypeError ||
    error instanceof VideoSourceUnavailableError ||
    error instanceof VideoTransferTimeoutError
  );
}

async function sendBytes(
  client: VideoImportPortClient,
  uploadId: string,
  firstSequence: number,
  bytes: Uint8Array,
): Promise<number> {
  let sequence = firstSequence;
  for (let offset = 0; offset < bytes.byteLength; offset += MAX_VIDEO_IMPORT_CHUNK_BYTES) {
    const chunk = bytes.subarray(
      offset,
      Math.min(offset + MAX_VIDEO_IMPORT_CHUNK_BYTES, bytes.byteLength),
    );
    const request: VideoImportChunkRequest = {
      protocol_version: PROTOCOL_VERSION,
      action: "video_import_chunk",
      upload_id: uploadId,
      sequence,
      data_base64: encodeBase64(chunk),
    };
    await requireAck(client.request(request));
    sequence += 1;
  }
  return sequence;
}

async function requireAck(responsePromise: Promise<VideoImportResponse>): Promise<void> {
  const response = await responsePromise;
  if (!response.ok) throw new VideoImportError(response.error, response.message || response.error);
}

function parseExpectedBytes(value: string | null): number | undefined {
  if (!value || !/^\d+$/.test(value)) return undefined;
  const bytes = Number(value);
  return Number.isSafeInteger(bytes) ? bytes : undefined;
}

function importMetadata(metadata: SaveRequestMetadata): VideoImportMetadata {
  const retained = { ...metadata };
  delete retained.media_url;
  return { ...retained, kind: "video" } as VideoImportMetadata;
}

async function readInitialVideoChunks(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  declared: string | null,
  signal: AbortSignal,
): Promise<{ chunks: Uint8Array[]; signature: Uint8Array }> {
  const chunks: Uint8Array[] = [];
  const signature = new Uint8Array(12);
  let signatureBytes = 0;
  const needsSniffing = isGenericContentType(declared);

  while (true) {
    const next = await readVideoChunk(reader, signal);
    if (next.done) break;
    if (!next.value?.byteLength) continue;
    chunks.push(next.value);

    if (signatureBytes < signature.byteLength) {
      const copied = Math.min(signature.byteLength - signatureBytes, next.value.byteLength);
      signature.set(next.value.subarray(0, copied), signatureBytes);
      signatureBytes += copied;
    }

    if (!needsSniffing || sniffVideoContentType(signature.subarray(0, signatureBytes))) break;
    if (signatureBytes === signature.byteLength) break;
  }

  if (chunks.length === 0) {
    throw new VideoSourceUnavailableError("The temporary video was empty.");
  }
  return { chunks, signature: signature.subarray(0, signatureBytes) };
}

function resolveVideoContentType(declared: string | null, bytes: Uint8Array): string {
  const normalized = declared?.split(";", 1)[0].trim().toLowerCase();
  if (normalized?.startsWith("video/")) return normalized;

  if (isGenericContentType(declared)) {
    const sniffed = sniffVideoContentType(bytes);
    if (sniffed) return sniffed;
  }
  throw new VideoSourceUnavailableError("The temporary source did not contain a supported video.");
}

function isGenericContentType(declared: string | null): boolean {
  const normalized = declared?.split(";", 1)[0].trim().toLowerCase();
  return (
    !normalized || normalized === "application/octet-stream" || normalized === "binary/octet-stream"
  );
}

function sniffVideoContentType(bytes: Uint8Array): string | undefined {
  if (bytes.byteLength >= 12 && ascii(bytes, 4, 8) === "ftyp") {
    return ascii(bytes, 8, 12) === "qt  " ? "video/quicktime" : "video/mp4";
  }
  if (
    bytes.byteLength >= 4 &&
    bytes[0] === 0x1a &&
    bytes[1] === 0x45 &&
    bytes[2] === 0xdf &&
    bytes[3] === 0xa3
  ) {
    return "video/webm";
  }
  return undefined;
}

function ascii(bytes: Uint8Array, start: number, end: number): string {
  return String.fromCharCode(...bytes.subarray(start, end));
}

function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  const stride = 0x8000;
  for (let offset = 0; offset < bytes.byteLength; offset += stride) {
    binary += String.fromCharCode(
      ...bytes.subarray(offset, Math.min(offset + stride, bytes.byteLength)),
    );
  }
  return btoa(binary);
}

class VideoImportPortClient {
  private readonly cancellation = new AbortController();
  private pending:
    | {
        resolve: (response: VideoImportResponse) => void;
        reject: (error: Error) => void;
        timer: ReturnType<typeof setTimeout>;
      }
    | undefined;
  private closed = false;
  private terminalError: VideoImportError | undefined;

  constructor(private readonly port: VideoImportPort) {
    port.onMessage.addListener(this.handleMessage);
    port.onDisconnect.addListener(this.handleDisconnect);
  }

  get signal(): AbortSignal {
    return this.cancellation.signal;
  }

  cancelSource(reason: unknown): void {
    this.cancellation.abort(reason);
  }

  preparing(uploadId: string): void {
    if (this.closed || this.pending || this.signal.aborted) return;
    try {
      this.port.postMessage({
        protocol_version: PROTOCOL_VERSION,
        action: "video_import_prepare",
        upload_id: uploadId,
      });
    } catch (error) {
      this.terminalError = new VideoImportError(
        "native_connection",
        error instanceof Error ? error.message : String(error),
      );
      this.cancelSource(this.terminalError);
    }
  }

  request(message: VideoImportRequest): Promise<VideoImportResponse> {
    if (this.terminalError) return Promise.reject(this.terminalError);
    if (this.closed) {
      return Promise.reject(
        new VideoImportError("native_connection", "The video import connection closed."),
      );
    }
    if (this.pending)
      return Promise.reject(new Error("A video import request is already pending."));

    return new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => {
          this.pending = undefined;
          this.terminalError = new VideoImportError(
            "video_timeout",
            "The Óia app stopped responding to the video import.",
          );
          this.cancelSource(this.terminalError);
          reject(this.terminalError);
          this.close();
        },
        message.action === "video_import_finish"
          ? VIDEO_TRANSFER_FINISH_TIMEOUT_MS
          : VIDEO_TRANSFER_TIMEOUT_MS,
      );
      this.pending = { resolve, reject, timer };
      try {
        this.port.postMessage(message);
      } catch (error) {
        clearTimeout(timer);
        this.pending = undefined;
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  abort(uploadId: string): void {
    if (this.closed || this.pending) return;
    const request: VideoImportAbortRequest = {
      protocol_version: PROTOCOL_VERSION,
      action: "video_import_abort",
      upload_id: uploadId,
    };
    try {
      this.port.postMessage(request);
    } catch {
      // The relay may already be gone; its disconnect handler also aborts a
      // native upload that had reached the host.
    }
  }

  close(): void {
    this.port.onMessage.removeListener(this.handleMessage);
    this.port.onDisconnect.removeListener(this.handleDisconnect);
    if (this.pending) clearTimeout(this.pending.timer);
    this.pending?.reject(new Error("The video import connection closed."));
    this.pending = undefined;
    this.cancelSource(this.terminalError ?? new Error("The video import connection closed."));
    if (this.closed) return;
    this.closed = true;
    this.port.disconnect();
  }

  private readonly handleMessage = (response: VideoImportResponse): void => {
    const pending = this.pending;
    if (!pending) {
      if (!response.ok) {
        this.terminalError = new VideoImportError(
          response.error,
          response.message || response.error,
        );
        this.cancelSource(this.terminalError);
      }
      return;
    }
    clearTimeout(pending.timer);
    this.pending = undefined;
    pending.resolve(response);
  };

  private readonly handleDisconnect = (): void => {
    if (this.closed) return;
    this.closed = true;
    this.port.onMessage.removeListener(this.handleMessage);
    this.port.onDisconnect.removeListener(this.handleDisconnect);
    if (this.pending) clearTimeout(this.pending.timer);
    this.terminalError ??= new VideoImportError(
      "native_connection",
      "The Óia app closed the video import connection.",
    );
    this.cancelSource(this.terminalError);
    this.pending?.reject(this.terminalError);
    this.pending = undefined;
  };
}
