// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import { servePageVideoSource } from "./page-video-main.js";
import { VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS, VIDEO_TRANSFER_TIMEOUT_MS } from "./video-transfer.js";

afterEach(() => vi.useRealTimers());

class FakePageVideoPort {
  readonly messages: unknown[] = [];
  readonly transfers: Transferable[][] = [];
  onmessage: ((event: MessageEvent<unknown>) => void) | null = null;
  started = false;
  closed = false;

  postMessage(message: unknown, transfer: Transferable[] = []): void {
    this.messages.push(message);
    this.transfers.push(transfer);
  }

  start(): void {
    this.started = true;
  }

  close(): void {
    this.closed = true;
  }

  send(message: unknown): void {
    this.onmessage?.(new MessageEvent("message", { data: message }));
  }
}

describe("MAIN-world page video source", () => {
  it("records an exact HTTP video element when direct fetching is unavailable", async () => {
    const videoUrl = "https://media.example.net/video.mp4";
    const bytes = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
    ]);
    let read = false;
    const recordedResponse = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: vi.fn(async () => {
            if (read) return { done: true as const, value: undefined };
            read = true;
            return { done: false as const, value: bytes };
          }),
          cancel: vi.fn(async () => undefined),
          releaseLock: vi.fn(),
        }),
      },
    } as unknown as Response;
    const fetchSource = vi.fn(async () => {
      throw new TypeError("Failed to fetch");
    });
    const recordSource = vi.fn(async () => recordedResponse);
    const port = new FakePageVideoPort();

    await servePageVideoSource(
      videoUrl,
      port,
      "https://example.com",
      fetchSource as typeof fetch,
      recordSource,
    );

    expect(fetchSource).toHaveBeenCalledWith(videoUrl, { signal: expect.any(AbortSignal) });
    expect(recordSource).toHaveBeenCalledWith(videoUrl, expect.any(AbortSignal));
    expect(port.messages[0]).toMatchObject({ type: "ready", contentType: "video/mp4" });
    port.send({ type: "next" });
    await vi.waitFor(() => expect(port.messages).toHaveLength(2));
    expect(new Uint8Array((port.messages[1] as { data: ArrayBuffer }).data)).toEqual(bytes);
  });

  it("uses a MAIN-world HTTP fetch before falling back to recording", async () => {
    const videoUrl = "https://media.example.net/page-readable.mp4";
    const bytes = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
    ]);
    let read = false;
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: vi.fn(async () => {
            if (read) return { done: true as const, value: undefined };
            read = true;
            return { done: false as const, value: bytes };
          }),
          cancel: vi.fn(async () => undefined),
          releaseLock: vi.fn(),
        }),
      },
    } as unknown as Response;
    const fetchSource = vi.fn(async () => response);
    const recordSource = vi.fn(async () => {
      throw new Error("Recording should not be needed");
    });
    const port = new FakePageVideoPort();

    await servePageVideoSource(videoUrl, port, "https://example.com", fetchSource, recordSource);

    expect(fetchSource).toHaveBeenCalledWith(videoUrl, { signal: expect.any(AbortSignal) });
    expect(recordSource).not.toHaveBeenCalled();
    expect(port.messages[0]).toMatchObject({ type: "ready", contentType: "video/mp4" });
    port.send({ type: "next" });
    await vi.waitFor(() => expect(port.messages).toHaveLength(2));
    expect(new Uint8Array((port.messages[1] as { data: ArrayBuffer }).data)).toEqual(bytes);
  });

  it("records the exact MAIN-world video when its page fetch is empty", async () => {
    const bytes = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
    ]);
    const emptyResponse = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4", "Content-Length": "0" }),
      body: {
        getReader: () => ({
          read: vi.fn(async () => ({ done: true as const, value: undefined })),
          cancel: vi.fn(async () => undefined),
          releaseLock: vi.fn(),
        }),
      },
    } as unknown as Response;
    let recordedRead = false;
    const recordedResponse = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: vi.fn(async () => {
            if (recordedRead) return { done: true as const, value: undefined };
            recordedRead = true;
            return { done: false as const, value: bytes };
          }),
          cancel: vi.fn(async () => undefined),
          releaseLock: vi.fn(),
        }),
      },
    } as unknown as Response;
    const recordSource = vi.fn(async () => recordedResponse);
    const port = new FakePageVideoPort();

    await servePageVideoSource(
      "blob:https://www.cosmos.so/media-source",
      port,
      "https://www.cosmos.so",
      async () => emptyResponse,
      recordSource,
    );

    expect(recordSource).toHaveBeenCalledWith(
      "blob:https://www.cosmos.so/media-source",
      expect.any(AbortSignal),
    );
    expect(port.messages[0]).toMatchObject({
      type: "ready",
      ok: true,
      contentType: "video/mp4",
    });
    port.send({ type: "next" });
    await vi.waitFor(() => expect(port.messages).toHaveLength(2));
    expect(new Uint8Array((port.messages[1] as { data: ArrayBuffer }).data)).toEqual(bytes);
  });

  it("probes one bounded chunk, then reads only when the content stream requests more", async () => {
    const chunks = [Uint8Array.from([1, 2, 3]), Uint8Array.from([4, 5])];
    let index = 0;
    const read = vi.fn(async () => {
      const value = chunks[index++];
      return value ? { done: false as const, value } : { done: true as const, value: undefined };
    });
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4", "Content-Length": "5" }),
      body: { getReader: () => ({ read, cancel: vi.fn(), releaseLock: vi.fn() }) },
    } as unknown as Response;
    const fetchSource = vi.fn(async () => response);
    const port = new FakePageVideoPort();

    await servePageVideoSource(
      "blob:https://www.cosmos.so/exact-video",
      port,
      "https://www.cosmos.so",
      fetchSource,
    );

    expect(port.started).toBe(true);
    expect(fetchSource).toHaveBeenCalledWith("blob:https://www.cosmos.so/exact-video", {
      signal: expect.any(AbortSignal),
    });
    expect(read).toHaveBeenCalledTimes(1);
    expect(port.messages).toEqual([
      { type: "ready", ok: true, contentType: "video/mp4", expectedBytes: 5 },
    ]);

    port.send({ type: "next" });
    await vi.waitFor(() => expect(port.messages).toHaveLength(2));
    expect(new Uint8Array((port.messages[1] as { data: ArrayBuffer }).data)).toEqual(chunks[0]);
    expect(port.transfers[1]).toHaveLength(1);
    expect(read).toHaveBeenCalledTimes(1);

    port.send({ type: "next" });
    await vi.waitFor(() => expect(port.messages).toHaveLength(3));
    expect(new Uint8Array((port.messages[2] as { data: ArrayBuffer }).data)).toEqual(chunks[1]);
    expect(read).toHaveBeenCalledTimes(2);

    port.send({ type: "next" });
    await vi.waitFor(() => expect(port.messages).toHaveLength(4));
    expect(port.messages[3]).toEqual({ type: "done" });
    expect(port.closed).toBe(true);
  });

  it("reports an unreadable MediaSource when MAIN-world recording also fails", async () => {
    const port = new FakePageVideoPort();
    const recordSource = vi.fn(async () => {
      throw new Error("The temporary video element is no longer on this page.");
    });

    await servePageVideoSource(
      "blob:https://www.cosmos.so/media-source",
      port,
      "https://www.cosmos.so",
      async () => {
        throw new TypeError("Failed to fetch");
      },
      recordSource,
    );

    expect(recordSource).toHaveBeenCalledWith(
      "blob:https://www.cosmos.so/media-source",
      expect.any(AbortSignal),
    );
    expect(port.messages).toEqual([
      {
        type: "error",
        ok: false,
        message: "The temporary video element is no longer on this page.",
      },
    ]);
    expect(port.closed).toBe(true);
  });

  it("caps every transferred chunk while retaining unread source bytes for the next pull", async () => {
    const chunkLimit = 256 * 1024;
    const bytes = Uint8Array.from({ length: chunkLimit + 3 }, (_, index) => index % 251);
    let readCount = 0;
    const read = vi.fn(async () => {
      readCount += 1;
      return readCount === 1
        ? { done: false as const, value: bytes }
        : { done: true as const, value: undefined };
    });
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: { getReader: () => ({ read, cancel: vi.fn(), releaseLock: vi.fn() }) },
    } as unknown as Response;
    const port = new FakePageVideoPort();

    await servePageVideoSource(
      "blob:https://www.cosmos.so/large-source-chunk",
      port,
      "https://www.cosmos.so",
      async () => response,
    );

    port.send({ type: "next" });
    await vi.waitFor(() => expect(port.messages).toHaveLength(2));
    const first = new Uint8Array((port.messages[1] as { data: ArrayBuffer }).data);
    expect(first.byteLength).toBe(chunkLimit);
    expect(read).toHaveBeenCalledTimes(1);

    port.send({ type: "next" });
    await vi.waitFor(() => expect(port.messages).toHaveLength(3));
    const second = new Uint8Array((port.messages[2] as { data: ArrayBuffer }).data);
    expect(Array.from(second)).toEqual(Array.from(bytes.subarray(chunkLimit)));
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("cancels the opened source when the content stream disconnects", async () => {
    const cancel = vi.fn(async () => undefined);
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: vi.fn(async () => ({ done: false as const, value: Uint8Array.from([1, 2, 3]) })),
          cancel,
          releaseLock: vi.fn(),
        }),
      },
    } as unknown as Response;
    const port = new FakePageVideoPort();

    await servePageVideoSource(
      "blob:https://www.cosmos.so/cancelled-source",
      port,
      "https://www.cosmos.so",
      async () => response,
    );
    port.send({ type: "cancel" });

    await vi.waitFor(() => expect(cancel).toHaveBeenCalledTimes(1));
    expect(port.closed).toBe(true);
  });

  it("rejects a blob URL owned by another origin before fetching", async () => {
    const port = new FakePageVideoPort();
    const fetchSource = vi.fn();

    await servePageVideoSource(
      "blob:https://attacker.example/video",
      port,
      "https://www.cosmos.so",
      fetchSource as typeof fetch,
    );

    expect(fetchSource).not.toHaveBeenCalled();
    expect(port.messages).toEqual([
      {
        type: "error",
        ok: false,
        message: "The temporary video does not belong to this page.",
      },
    ]);
  });

  it("accepts cancellation before a stalled fetch has opened", async () => {
    vi.useFakeTimers();
    const port = new FakePageVideoPort();
    let signal: AbortSignal | undefined;
    const fetchSource = vi.fn((_url: unknown, options?: RequestInit) => {
      signal = options?.signal ?? undefined;
      return new Promise<Response>(() => undefined);
    });
    const recordSource = vi.fn();
    const serving = servePageVideoSource(
      "https://example.com/stalled.mp4",
      port,
      "https://example.com",
      fetchSource as typeof fetch,
      recordSource,
    );
    await vi.advanceTimersByTimeAsync(0);

    port.send({ type: "cancel" });
    await serving;
    expect(signal?.aborted).toBe(true);
    expect(port.closed).toBe(true);
    expect(recordSource).not.toHaveBeenCalled();
    expect(port.messages).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("falls back to recording after the MAIN fetch deadline", async () => {
    vi.useFakeTimers();
    const port = new FakePageVideoPort();
    const recordSource = vi.fn(
      async () =>
        new Response(Uint8Array.from([1]), {
          headers: { "Content-Type": "video/mp4" },
        }),
    );
    const serving = servePageVideoSource(
      "https://example.com/stalled.mp4",
      port,
      "https://example.com",
      () => new Promise(() => undefined),
      recordSource,
    );

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    await serving;
    expect(recordSource).toHaveBeenCalledTimes(1);
    expect(port.messages[0]).toMatchObject({ type: "ready", contentType: "video/mp4" });
    port.send({ type: "cancel" });
    await vi.advanceTimersByTimeAsync(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cancels and releases a stalled body without waiting forever for cancellation", async () => {
    vi.useFakeTimers();
    const port = new FakePageVideoPort();
    let first = true;
    const cancel = vi.fn(() => new Promise<void>(() => undefined));
    const releaseLock = vi.fn();
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: () => {
            if (!first) return new Promise(() => undefined);
            first = false;
            return Promise.resolve({ done: false, value: Uint8Array.from([1]) });
          },
          cancel,
          releaseLock,
        }),
      },
    } as unknown as Response;
    await servePageVideoSource(
      "https://example.com/body.mp4",
      port,
      "https://example.com",
      async () => response,
    );
    port.send({ type: "next" });
    await vi.advanceTimersByTimeAsync(0);
    port.send({ type: "next" });

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    expect(port.closed).toBe(true);
    expect(port.messages.at(-1)).toMatchObject({ type: "error", ok: false });
    expect(cancel).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS);
    expect(releaseLock).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("releases an opened source if the content endpoint disappears without cancelling", async () => {
    vi.useFakeTimers();
    const port = new FakePageVideoPort();
    const cancel = vi.fn(async () => undefined);
    const releaseLock = vi.fn();
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: async () => ({ done: false, value: Uint8Array.from([1]) }),
          cancel,
          releaseLock,
        }),
      },
    } as unknown as Response;
    await servePageVideoSource(
      "https://example.com/idle.mp4",
      port,
      "https://example.com",
      async () => response,
    );

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    expect(port.closed).toBe(true);
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(releaseLock).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });
});
