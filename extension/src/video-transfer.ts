// SPDX-License-Identifier: MIT

/** These are inactivity limits, never a limit on a progressing video's length. */
export const VIDEO_TRANSFER_TIMEOUT_MS = 60_000;
export const VIDEO_TRANSFER_FINISH_TIMEOUT_MS = 5 * 60_000;
export const VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS = 5_000;
export const VIDEO_PREPARATION_HEARTBEAT_MS = 20_000;
export const VIDEO_PREPARATION_TIMEOUT_MS = 10 * 60_000;
// MAIN may try a fetch and then a recorder, each waiting for a response and its
// first bytes. Leave those bounded attempts time to finish before abandoning it.
export const PAGE_VIDEO_READY_TIMEOUT_MS =
  4 * VIDEO_TRANSFER_TIMEOUT_MS + VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS;

export class VideoTransferTimeoutError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "VideoTransferTimeoutError";
  }
}

export function waitForVideoTransfer<T>(
  pending: Promise<T>,
  message: string,
  timeoutMs = VIDEO_TRANSFER_TIMEOUT_MS,
  signal?: AbortSignal,
  onTimeout?: (error: Error) => void,
): Promise<T> {
  return new Promise((resolve, reject) => {
    const cleanup = (): void => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", aborted);
    };
    const aborted = (): void => {
      cleanup();
      reject(signal?.reason ?? new Error("The video transfer was cancelled."));
    };
    const timer = setTimeout(() => {
      cleanup();
      const error = new VideoTransferTimeoutError(message);
      reject(error);
      onTimeout?.(error);
    }, timeoutMs);
    // Always observe the underlying promise, including after timeout or abort.
    pending.then(
      (value) => {
        cleanup();
        resolve(value);
      },
      (error: unknown) => {
        cleanup();
        reject(error);
      },
    );
    signal?.addEventListener("abort", aborted, { once: true });
    if (signal?.aborted) aborted();
  });
}

/** Abort real fetches/recorders and cancel a late custom opener's response too. */
export async function openVideoResponse(
  open: (signal: AbortSignal) => Promise<Response>,
  signal?: AbortSignal,
  timeoutMs = VIDEO_TRANSFER_TIMEOUT_MS,
): Promise<Response> {
  const controller = new AbortController();
  const abort = (): void => controller.abort(signal?.reason);
  signal?.addEventListener("abort", abort, { once: true });
  if (signal?.aborted) abort();
  const pending = Promise.resolve().then(() => {
    controller.signal.throwIfAborted();
    return open(controller.signal);
  });
  void pending.then(
    (response) => {
      if (controller.signal.aborted) {
        void cancelVideoResponse(response, controller.signal.reason);
      }
    },
    () => undefined,
  );
  try {
    return await waitForVideoTransfer(
      pending,
      "The video source stopped responding while it was opening.",
      timeoutMs,
      controller.signal,
      (error) => controller.abort(error),
    );
  } finally {
    signal?.removeEventListener("abort", abort);
  }
}

/** Empty chunks do not count as progress or restart the inactivity clock. */
export function readVideoChunk(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  signal?: AbortSignal,
): Promise<ReadableStreamReadResult<Uint8Array>> {
  return waitForVideoTransfer(
    (async () => {
      while (true) {
        signal?.throwIfAborted();
        const next = await reader.read();
        if (next.done || next.value?.byteLength) return next;
      }
    })(),
    "The video source stopped sending data.",
    VIDEO_TRANSFER_TIMEOUT_MS,
    signal,
  );
}

export async function cancelVideoReader(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  reason?: unknown,
): Promise<void> {
  try {
    // Older focused stream fakes do not expose cancel; real readers always do.
    if (typeof reader.cancel === "function") {
      await waitForVideoTransfer(
        Promise.resolve().then(() => reader.cancel(reason)),
        "The video source did not finish cancelling.",
        VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS,
      );
    }
  } catch {
    // Cleanup must not obscure the transfer error or retain its port forever.
  } finally {
    releaseVideoReader(reader);
  }
}

export function releaseVideoReader(reader: ReadableStreamDefaultReader<Uint8Array>): void {
  try {
    reader.releaseLock();
  } catch {
    // Some hosts cannot release a reader until its pending read is cancelled.
  }
}

async function cancelVideoResponse(response: Response, reason?: unknown): Promise<void> {
  try {
    await waitForVideoTransfer(
      response.body?.cancel(reason) ?? Promise.resolve(),
      "The video source did not finish cancelling.",
      VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS,
    );
  } catch {
    // A late response is no longer part of the transfer.
  }
}
