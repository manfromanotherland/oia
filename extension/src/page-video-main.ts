// SPDX-License-Identifier: MIT

import { OPEN_PAGE_VIDEO_SOURCE } from "./page-video-source.js";
import { recordVideoElement } from "./video-element-recorder.js";
import {
  cancelVideoReader,
  openVideoResponse,
  PAGE_VIDEO_READY_TIMEOUT_MS,
  readVideoChunk,
  releaseVideoReader,
  VIDEO_TRANSFER_TIMEOUT_MS,
  VideoTransferTimeoutError,
} from "./video-transfer.js";

const PAGE_VIDEO_BRIDGE_MARKER = "__oiaPageVideoBridgeV1";
const MAX_PAGE_VIDEO_CHUNK_BYTES = 256 * 1024;

interface PageVideoPort {
  postMessage(message: unknown, transfer?: Transferable[]): void;
  close(): void;
  start(): void;
  onmessage: ((event: MessageEvent<unknown>) => void) | null;
}

interface PageVideoBridgeWindow extends Window {
  [PAGE_VIDEO_BRIDGE_MARKER]?: true;
}

/** Install the idempotent MAIN-world listener used by the content-side stream. */
export function installPageVideoBridge(
  targetWindow: PageVideoBridgeWindow = window,
  fetchSource: typeof fetch = fetch,
  recordSource: (url: string, signal?: AbortSignal) => Promise<Response> = (url, signal) =>
    recordVideoElement(targetWindow.document, url, { signal }),
): void {
  if (targetWindow[PAGE_VIDEO_BRIDGE_MARKER]) return;
  targetWindow[PAGE_VIDEO_BRIDGE_MARKER] = true;

  targetWindow.addEventListener("message", (event) => {
    if (event.source !== targetWindow) return;
    if (!isOpenRequest(event.data)) return;
    const port = event.ports[0] as PageVideoPort | undefined;
    if (!port) return;
    void servePageVideoSource(
      event.data.url,
      port,
      targetWindow.location.origin,
      fetchSource,
      recordSource,
    );
  });
}

/** Serve exactly one selected page video with pull-based backpressure. */
export async function servePageVideoSource(
  url: string,
  port: PageVideoPort,
  pageOrigin: string,
  fetchSource: typeof fetch,
  recordSource?: (url: string, signal?: AbortSignal) => Promise<Response>,
): Promise<void> {
  port.start();
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
  let pendingBytes: Uint8Array | undefined;
  let pendingOffset = 0;
  let reading = false;
  let terminal = false;
  const cancellation = new AbortController();
  let idleTimer: ReturnType<typeof setTimeout>;

  const close = (reason?: unknown): void => {
    if (terminal) return;
    terminal = true;
    clearTimeout(idleTimer);
    cancellation.abort(reason);
    port.onmessage = null;
    port.close();
    if (reader) {
      if (reason === undefined) releaseVideoReader(reader);
      else void cancelVideoReader(reader, reason);
    }
  };
  const fail = (error: unknown): void => {
    if (terminal) return;
    try {
      port.postMessage({
        type: "error",
        ok: false,
        message: error instanceof Error ? error.message : String(error),
      });
    } catch {
      // The isolated endpoint may have disappeared with its document.
    } finally {
      close(error);
    }
  };
  const armIdle = (timeoutMs = VIDEO_TRANSFER_TIMEOUT_MS): void => {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(
      () => fail(new VideoTransferTimeoutError("The page video capture stopped making progress.")),
      timeoutMs,
    );
  };
  armIdle(PAGE_VIDEO_READY_TIMEOUT_MS);

  // Cancellation must be accepted while fetch, first bytes, or play() is still
  // pending, not only after the source has announced that it is ready.
  port.onmessage = (event): void => {
    const request = event.data as { type?: unknown } | null;
    if (request?.type === "cancel") {
      close(new Error("The page video capture was cancelled."));
      return;
    }
    if (request?.type !== "next" || reading || terminal || !reader) return;
    armIdle();
    reading = true;
    void nextTransferableChunk(reader, () => ({ pendingBytes, pendingOffset }), cancellation.signal)
      .then((next) => {
        if (terminal) return;
        pendingBytes = next.pendingBytes;
        pendingOffset = next.pendingOffset;
        if (!next.data) {
          port.postMessage({ type: "done" });
          close();
          return;
        }
        port.postMessage({ type: "chunk", data: next.data }, [next.data]);
        armIdle();
      })
      .catch(fail)
      .finally(() => {
        reading = false;
      });
  };

  try {
    const parsed = new URL(url);
    if (parsed.protocol === "blob:" && parsed.origin !== pageOrigin) {
      throw new Error("The temporary video does not belong to this page.");
    }
    if (!isRecordableVideoProtocol(parsed.protocol)) {
      throw new Error("The page video uses an unsupported source.");
    }

    const opened = await openPageVideoSource(url, fetchSource, recordSource, cancellation.signal);
    if (terminal) {
      void cancelVideoReader(opened.reader, cancellation.signal.reason);
      return;
    }
    reader = opened.reader;
    pendingBytes = opened.firstBytes;
    port.postMessage({
      type: "ready",
      ok: true,
      contentType: opened.contentType,
      ...(opened.expectedBytes === undefined ? {} : { expectedBytes: opened.expectedBytes }),
    });

    armIdle();
  } catch (error) {
    fail(error);
  }
}

interface OpenedPageVideoSource {
  reader: ReadableStreamDefaultReader<Uint8Array>;
  firstBytes: Uint8Array;
  contentType: string;
  expectedBytes?: number;
}

async function openPageVideoSource(
  url: string,
  fetchSource: typeof fetch,
  recordSource: ((url: string, signal?: AbortSignal) => Promise<Response>) | undefined,
  signal: AbortSignal,
): Promise<OpenedPageVideoSource> {
  try {
    return await openNonEmptyResponse(
      await openVideoResponse((fetchSignal) => fetchSource(url, { signal: fetchSignal }), signal),
      signal,
    );
  } catch {
    signal.throwIfAborted();
    return openNonEmptyResponse(
      await openVideoResponse(
        (recordingSignal) =>
          recordSource
            ? recordSource(url, recordingSignal)
            : recordVideoElement(document, url, { signal: recordingSignal }),
        signal,
      ),
      signal,
    );
  }
}

async function openNonEmptyResponse(
  response: Response,
  signal: AbortSignal,
): Promise<OpenedPageVideoSource> {
  if (!response.ok || !response.body) {
    throw new Error("The temporary video could not be read from this page.");
  }

  const reader = response.body.getReader();
  try {
    while (true) {
      const first = await readVideoChunk(reader, signal);
      if (first.done) throw new Error("The temporary video was empty.");
      if (!first.value.byteLength) continue;
      return {
        reader,
        firstBytes: first.value,
        contentType: response.headers.get("Content-Type") || "application/octet-stream",
        expectedBytes: exactContentLength(response.headers.get("Content-Length")),
      };
    }
  } catch (error) {
    await cancelVideoReader(reader, error);
    throw error;
  }
}

interface PendingSourceBytes {
  pendingBytes: Uint8Array | undefined;
  pendingOffset: number;
}

interface TransferableChunk extends PendingSourceBytes {
  data?: ArrayBuffer;
}

async function nextTransferableChunk(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  pending: () => PendingSourceBytes,
  signal: AbortSignal,
): Promise<TransferableChunk> {
  let { pendingBytes, pendingOffset } = pending();

  while (true) {
    if (pendingBytes && pendingOffset < pendingBytes.byteLength) {
      const end = Math.min(pendingOffset + MAX_PAGE_VIDEO_CHUNK_BYTES, pendingBytes.byteLength);
      const data = pendingBytes.slice(pendingOffset, end).buffer as ArrayBuffer;
      pendingOffset = end;
      if (pendingOffset === pendingBytes.byteLength) {
        pendingBytes = undefined;
        pendingOffset = 0;
      }
      return { data, pendingBytes, pendingOffset };
    }

    const next = await readVideoChunk(reader, signal);
    if (next.done) return { pendingBytes: undefined, pendingOffset: 0 };
    if (!next.value.byteLength) continue;
    pendingBytes = next.value;
    pendingOffset = 0;
  }
}

function isOpenRequest(value: unknown): value is { type: string; url: string } {
  if (!value || typeof value !== "object") return false;
  const request = value as { type?: unknown; url?: unknown };
  return request.type === OPEN_PAGE_VIDEO_SOURCE && typeof request.url === "string";
}

function isRecordableVideoProtocol(protocol: string): boolean {
  return (
    protocol === "blob:" || protocol === "http:" || protocol === "https:" || protocol === "data:"
  );
}

function exactContentLength(value: string | null): number | undefined {
  if (!value || !/^\d+$/.test(value)) return undefined;
  const bytes = Number(value);
  return Number.isSafeInteger(bytes) ? bytes : undefined;
}

installPageVideoBridge();
