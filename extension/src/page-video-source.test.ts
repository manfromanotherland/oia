// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import { OPEN_PAGE_VIDEO_SOURCE, fetchPageVideoSource } from "./page-video-source.js";
import { PAGE_VIDEO_READY_TIMEOUT_MS, VIDEO_TRANSFER_TIMEOUT_MS } from "./video-transfer.js";

class FakeMessagePort extends EventTarget {
  peer?: FakeMessagePort;
  closed = false;
  readonly messages: unknown[] = [];

  start(): void {}
  close(): void {
    this.closed = true;
  }
  postMessage(data: unknown): void {
    this.messages.push(data);
    this.peer?.dispatchEvent(new MessageEvent("message", { data }));
  }
}

function stubMessageChannel(): FakeMessagePort[] {
  const ports: FakeMessagePort[] = [];
  vi.stubGlobal(
    "MessageChannel",
    class {
      readonly port1 = new FakeMessagePort();
      readonly port2 = new FakeMessagePort();
      constructor() {
        this.port1.peer = this.port2;
        this.port2.peer = this.port1;
        ports.push(this.port1, this.port2);
      }
    },
  );
  return ports;
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("content-side page video source", () => {
  it("exposes the MAIN-world chunks as a pullable Response without whole-video buffering", async () => {
    const prepare = vi.fn(async () => undefined);
    const chunks = [Uint8Array.from([1, 2, 3]), Uint8Array.from([4, 5])];
    let index = 0;
    const targetWindow = {
      postMessage(message: unknown, _origin: string, transfer: Transferable[]) {
        expect(message).toEqual({
          type: OPEN_PAGE_VIDEO_SOURCE,
          url: "blob:https://www.cosmos.so/video",
        });
        const port = transfer[0] as MessagePort;
        port.start();
        port.postMessage({
          type: "ready",
          ok: true,
          contentType: "video/mp4",
          expectedBytes: 5,
        });
        port.onmessage = (event) => {
          if (event.data.type === "next") {
            const value = chunks[index++];
            if (!value) {
              port.postMessage({ type: "done" });
            } else {
              const data = value.slice().buffer;
              port.postMessage({ type: "chunk", data }, [data]);
            }
          }
        };
      },
    } as unknown as Window;

    const response = await fetchPageVideoSource(
      "blob:https://www.cosmos.so/video",
      prepare,
      targetWindow,
    );
    const reader = response.body!.getReader();
    const received: number[] = [];
    while (true) {
      const next = await reader.read();
      if (next.done) break;
      received.push(...next.value);
    }

    expect(prepare).toHaveBeenCalledTimes(1);
    expect(response.headers.get("Content-Type")).toBe("video/mp4");
    expect(response.headers.get("Content-Length")).toBe("5");
    expect(received).toEqual([1, 2, 3, 4, 5]);
  });

  it("times out bridge preparation instead of waiting for a missing runtime reply", async () => {
    vi.useFakeTimers();
    const targetWindow = { postMessage: vi.fn() } as unknown as Window;
    const pending = fetchPageVideoSource(
      "https://example.com/video.mp4",
      () => new Promise(() => undefined),
      targetWindow,
    );
    const rejected = expect(pending).rejects.toThrow("bridge did not respond");

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    await rejected;
    expect(targetWindow.postMessage).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("closes the channel and cancels MAIN when the ready reply never arrives", async () => {
    vi.useFakeTimers();
    const ports = stubMessageChannel();
    const pending = fetchPageVideoSource("https://example.com/video.mp4", async () => undefined, {
      postMessage: vi.fn(),
    } as unknown as Window);
    const rejected = expect(pending).rejects.toThrow("stopped responding");

    await vi.advanceTimersByTimeAsync(PAGE_VIDEO_READY_TIMEOUT_MS);
    await rejected;
    expect(ports[0].closed).toBe(true);
    expect(ports[0].messages).toEqual([{ type: "cancel" }]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("errors a pending body read and closes the channel when MAIN stops answering next", async () => {
    vi.useFakeTimers();
    const ports = stubMessageChannel();
    const targetWindow = {
      postMessage(_message: unknown, _origin: string, transfer: Transferable[]) {
        const port = transfer[0] as unknown as FakeMessagePort;
        port.postMessage({ type: "ready", ok: true, contentType: "video/mp4" });
      },
    } as unknown as Window;
    const response = await fetchPageVideoSource(
      "https://example.com/video.mp4",
      async () => undefined,
      targetWindow,
    );
    const reader = response.body!.getReader();
    const rejected = expect(reader.read()).rejects.toThrow("stopped responding");

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    await rejected;
    expect(ports[0].messages).toEqual([{ type: "next" }, { type: "cancel" }]);
    expect(ports[0].closed).toBe(true);
    reader.releaseLock();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("removes a pending reply listener when the caller cancels the body", async () => {
    vi.useFakeTimers();
    const ports = stubMessageChannel();
    const targetWindow = {
      postMessage(_message: unknown, _origin: string, transfer: Transferable[]) {
        (transfer[0] as unknown as FakeMessagePort).postMessage({
          type: "ready",
          ok: true,
          contentType: "video/mp4",
        });
      },
    } as unknown as Window;
    const response = await fetchPageVideoSource(
      "https://example.com/video.mp4",
      async () => undefined,
      targetWindow,
    );
    const reader = response.body!.getReader();
    const pending = reader.read();
    await vi.advanceTimersByTimeAsync(0);
    await reader.cancel();
    await expect(pending).resolves.toMatchObject({ done: true });
    expect(ports[0].closed).toBe(true);
    expect(ports[0].messages.at(-1)).toEqual({ type: "cancel" });
    reader.releaseLock();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("errors a pending body read when its opening caller is aborted", async () => {
    vi.useFakeTimers();
    const ports = stubMessageChannel();
    const cancellation = new AbortController();
    const targetWindow = {
      postMessage(_message: unknown, _origin: string, transfer: Transferable[]) {
        (transfer[0] as unknown as FakeMessagePort).postMessage({
          type: "ready",
          ok: true,
          contentType: "video/mp4",
        });
      },
    } as unknown as Window;
    const response = await fetchPageVideoSource(
      "https://example.com/video.mp4",
      async () => undefined,
      targetWindow,
      cancellation.signal,
    );
    const reader = response.body!.getReader();
    const rejected = expect(reader.read()).rejects.toThrow("capture cancelled");
    await vi.advanceTimersByTimeAsync(0);
    cancellation.abort(new Error("capture cancelled"));
    await rejected;

    expect(ports[0].closed).toBe(true);
    expect(ports[0].messages.at(-1)).toEqual({ type: "cancel" });
    reader.releaseLock();
    expect(vi.getTimerCount()).toBe(0);
  });
});
