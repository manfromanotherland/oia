// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  cancelVideoReader,
  openVideoResponse,
  readVideoChunk,
  VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS,
  VIDEO_TRANSFER_TIMEOUT_MS,
} from "./video-transfer.js";

afterEach(() => vi.useRealTimers());

describe("video transfer inactivity", () => {
  it("aborts a stalled opener and cancels a response that arrives after its timeout", async () => {
    vi.useFakeTimers();
    let signal: AbortSignal | undefined;
    let respond!: (response: Response) => void;
    const pending = openVideoResponse((openingSignal) => {
      signal = openingSignal;
      return new Promise((resolve) => {
        respond = resolve;
      });
    });
    const rejected = expect(pending).rejects.toThrow("stopped responding");

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    await rejected;
    expect(signal?.aborted).toBe(true);

    const cancel = vi.fn(async () => undefined);
    respond({ body: { cancel } } as unknown as Response);
    await vi.advanceTimersByTimeAsync(0);
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("releases a reader even when cancellation never settles", async () => {
    vi.useFakeTimers();
    const cancel = vi.fn(() => new Promise<void>(() => undefined));
    const releaseLock = vi.fn();
    const cancelled = cancelVideoReader({
      cancel,
      releaseLock,
    } as unknown as ReadableStreamDefaultReader<Uint8Array>);

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS);
    await cancelled;
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(releaseLock).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not let a trickle of empty chunks restart the no-data deadline", async () => {
    vi.useFakeTimers();
    const read = vi.fn(
      () =>
        new Promise<ReadableStreamReadResult<Uint8Array>>((resolve) => {
          setTimeout(() => resolve({ done: false, value: new Uint8Array() }), 40_000);
        }),
    );
    const pending = readVideoChunk({ read } as unknown as ReadableStreamDefaultReader<Uint8Array>);
    const rejected = expect(pending).rejects.toThrow("stopped sending data");

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    await rejected;
    expect(read).toHaveBeenCalledTimes(2);
  });
});
