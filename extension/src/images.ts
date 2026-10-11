// SPDX-License-Identifier: MIT

import type { ImageData } from "./protocol.js";

/** How many images to fetch at once. */
const MAX_CONCURRENCY = 6;

/** Abort a single image fetch that takes longer than this. */
const FETCH_TIMEOUT_MS = 15_000;

/** Bound one page asset before it is base64 encoded, matching the total envelope. */
const MAX_IMAGE_BYTES = 40 * 1024 * 1024;

/** Bound content-script → worker messages before native-message capping. */
const MAX_FETCH_TOTAL_BYTES = 40 * 1024 * 1024;

/** The outcome of fetching a set of image URLs. */
export interface FetchImagesResult {
  /** Images whose bytes we captured, ready to send to the host. */
  images: ImageData[];
  /** URLs we couldn't read (e.g. cross-origin without CORS) — a caller in a more
   *  privileged context (the background worker) can retry these. */
  unresolved: string[];
  /** URLs omitted by a byte budget. Retrying them would repeat the same work. */
  skipped: string[];
}

export interface FetchImagesOptions {
  maxTotalBytes?: number;
  maxImageBytes?: number;
  concurrency?: number;
}

/**
 * Fetch each distinct URL and return the bytes we could read.
 *
 * The same code runs in two contexts with different reach:
 * - In a **content script** (page origin) a `fetch` reuses the page's HTTP cache,
 *   so an image the browser already loaded comes back with no network request —
 *   readable when the response allows CORS (or is same-origin).
 * - In the **background service worker** (with host permissions) a `fetch` can
 *   read cross-origin bytes regardless of CORS, covering the rest.
 */
export async function fetchImages(
  urls: string[],
  options: FetchImagesOptions = {},
): Promise<FetchImagesResult> {
  const unique = [...new Set(urls)];
  const maxTotalBytes = byteLimit(options.maxTotalBytes ?? MAX_FETCH_TOTAL_BYTES);
  const maxImageBytes = byteLimit(options.maxImageBytes ?? MAX_IMAGE_BYTES);
  const requestedConcurrency = options.concurrency ?? MAX_CONCURRENCY;
  const concurrency = Number.isFinite(requestedConcurrency)
    ? Math.max(1, Math.min(MAX_CONCURRENCY, Math.floor(requestedConcurrency)))
    : MAX_CONCURRENCY;
  if (!maxTotalBytes || !maxImageBytes) return { images: [], unresolved: [], skipped: unique };
  const pending = new Map<number, PendingImage>();
  let next = 0;
  const images: ImageData[] = [];
  const unresolved: string[] = [];
  const skipped: string[] = [];
  let totalBytes = 0;

  const prefetch = (): void => {
    while (next < unique.length && pending.size < concurrency && totalBytes < maxTotalBytes) {
      pending.set(next, openImage(unique[next], maxImageBytes));
      next++;
    }
  };

  try {
    prefetch();
    for (let index = 0; index < unique.length; index++) {
      if (totalBytes >= maxTotalBytes) {
        skipped.push(...unique.slice(index));
        break;
      }
      const download = pending.get(index)!;
      // Fetch headers concurrently, but consume and encode bodies in candidate
      // order. Only one byte buffer exists, bounded by the remaining page budget.
      const outcome = await consumeImage(
        download,
        Math.min(maxImageBytes, maxTotalBytes - totalBytes),
      );
      pending.delete(index);
      if (outcome === "skipped") skipped.push(unique[index]);
      else if (!outcome) unresolved.push(unique[index]);
      else {
        totalBytes += decodedBase64Size(outcome.data_base64);
        images.push(outcome);
      }
      prefetch();
    }
  } finally {
    for (const download of pending.values()) download.cancel();
  }
  return { images, unresolved, skipped };
}

/** Fetch one image and encode it, or return null if it can't be read. */
export async function fetchImage(url: string): Promise<ImageData | null> {
  const result = await consumeImage(openImage(url, MAX_IMAGE_BYTES), MAX_IMAGE_BYTES);
  return result === "skipped" ? null : result;
}

interface OpenedImage {
  response: Response;
  content_type: string;
}

interface PendingImage {
  url: string;
  result: Promise<OpenedImage | "skipped" | null>;
  cancel: () => void;
  controller: AbortController;
}

function openImage(url: string, maxBytes: number): PendingImage {
  const controller = new AbortController();
  let openedResponse: Response | undefined;
  const timer = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  const cancel = (): void => {
    clearTimeout(timer);
    controller.abort();
    void openedResponse?.body?.cancel().catch(() => undefined);
  };
  const result = (async (): Promise<OpenedImage | "skipped" | null> => {
    try {
      const response = await fetch(url, { signal: controller.signal });
      openedResponse = response;
      if (controller.signal.aborted) {
        cancel();
        return null;
      }
      if (!response.ok || (response.url && !safeResponseUrl(url, response.url))) {
        cancel();
        return null;
      }
      const declaredLength = Number(response.headers.get("content-length"));
      if (Number.isFinite(declaredLength) && declaredLength > maxBytes) {
        cancel();
        return "skipped";
      }
      const content_type = (response.headers.get("content-type") ?? "").split(";")[0].trim();
      if (
        content_type &&
        !content_type.startsWith("image/") &&
        content_type !== "application/octet-stream"
      ) {
        cancel();
        return null;
      }
      return { response, content_type };
    } catch {
      cancel();
      return null;
    } finally {
      clearTimeout(timer);
    }
  })();
  return { url, result, cancel, controller };
}

async function consumeImage(
  download: PendingImage,
  maxBytes: number,
): Promise<ImageData | "skipped" | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const opened = await download.result;
    if (!opened || opened === "skipped") return opened;
    const { response, content_type } = opened;
    const declaredLength = Number(response.headers.get("content-length"));
    if (Number.isFinite(declaredLength) && declaredLength > maxBytes) return "skipped";
    timer = setTimeout(() => download.controller.abort(), FETCH_TIMEOUT_MS);
    const bytes = await readBoundedBytes(response, maxBytes);
    if (bytes === "skipped") return bytes;
    if (!bytes?.byteLength) return null;
    return { url: download.url, content_type, data_base64: bytesToBase64(bytes) };
  } catch {
    // Opaque/blocked by CORS, network error, timeout, or abort.
    return null;
  } finally {
    clearTimeout(timer);
    download.cancel();
  }
}

/** Base64-encode bytes, chunked so a large image can't overflow the call stack. */
export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

/**
 * Keep images in order until their cumulative decoded size would exceed
 * `maxBytes`; drop the rest (they'll stay as remote-URL placeholders). Bounds
 * how much we buffer and send in a single native message.
 */
export function capTotalBytes(images: ImageData[], maxBytes: number): ImageData[] {
  const kept: ImageData[] = [];
  let total = 0;
  for (const image of images) {
    // Decoded size is ~3/4 of the base64 length.
    const size = decodedBase64Size(image.data_base64);
    if (total + size > maxBytes) continue;
    total += size;
    kept.push(image);
  }
  return kept;
}

async function readBoundedBytes(
  response: Response,
  maxBytes: number,
): Promise<Uint8Array | "skipped" | null> {
  const reader = response.body?.getReader();
  if (!reader) {
    return null;
  }

  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > maxBytes) {
        await reader.cancel().catch(() => undefined);
        return "skipped";
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }

  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

function byteLimit(value: number): number {
  return Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0;
}

export function decodedBase64Size(value: string): number {
  const padding = value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0;
  return Math.max(0, Math.floor((value.length * 3) / 4) - padding);
}

function safeResponseUrl(requestedValue: string, responseValue: string): boolean {
  try {
    const requested = new URL(requestedValue);
    const response = new URL(responseValue);
    if (requested.protocol === "http:" || requested.protocol === "https:") {
      return (
        (response.protocol === "http:" || response.protocol === "https:") &&
        !response.username &&
        !response.password
      );
    }
    return (
      (requested.protocol === "blob:" || requested.protocol === "data:") &&
      response.protocol === requested.protocol
    );
  } catch {
    return false;
  }
}
