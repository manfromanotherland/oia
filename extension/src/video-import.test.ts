// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import { PROTOCOL_VERSION, type VideoImportResponse } from "./protocol.js";
import {
  importBlobVideo,
  importVideo,
  MAX_VIDEO_IMPORT_CHUNK_BYTES,
  VIDEO_IMPORT_PORT_NAME,
  type VideoImportPort,
  type VideoImportPortMessage,
} from "./video-import.js";
import {
  VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS,
  VIDEO_TRANSFER_FINISH_TIMEOUT_MS,
  VIDEO_TRANSFER_TIMEOUT_MS,
  VIDEO_PREPARATION_HEARTBEAT_MS,
  VIDEO_PREPARATION_TIMEOUT_MS,
} from "./video-transfer.js";

afterEach(() => vi.useRealTimers());

class ListenerEvent<T extends (...args: never[]) => void> {
  private readonly listeners = new Set<T>();

  addListener(listener: T): void {
    this.listeners.add(listener);
  }

  removeListener(listener: T): void {
    this.listeners.delete(listener);
  }

  emit(...args: Parameters<T>): void {
    for (const listener of this.listeners) listener(...args);
  }
}

class FakeVideoImportPort implements VideoImportPort {
  readonly onMessage = new ListenerEvent<(message: VideoImportResponse) => void>();
  readonly onDisconnect = new ListenerEvent<() => void>();
  readonly messages: VideoImportPortMessage[] = [];
  disconnected = false;

  postMessage(message: VideoImportPortMessage): void {
    this.messages.push(message);
  }

  disconnect(): void {
    this.disconnected = true;
  }

  acknowledge(response: VideoImportResponse): void {
    this.onMessage.emit(response);
  }

  drop(): void {
    this.onDisconnect.emit();
  }
}

class AutoAckVideoImportPort extends FakeVideoImportPort {
  override postMessage(message: VideoImportPortMessage): void {
    super.postMessage(message);
    queueMicrotask(() => {
      this.acknowledge(
        message.action === "video_import_finish"
          ? {
              protocol_version: PROTOCOL_VERSION,
              ok: true,
              id: "saved-video-id",
              path: "articles/sa/saved-video-id/article.md",
            }
          : { protocol_version: PROTOCOL_VERSION, ok: true },
      );
    });
  }
}

function cosmosDocument(blobUrl: string): Document {
  const doc = document.implementation.createHTMLDocument();
  doc.head.innerHTML = "<title>Cosmos</title>";
  doc.body.innerHTML = `<video data-testid="element-view-video" src="${blobUrl}"></video>`;
  return doc;
}

function byteRange(length: number, offset = 0): Uint8Array {
  return Uint8Array.from({ length }, (_, index) => (index + offset) % 251);
}

function decodedLength(base64: string): number {
  return Buffer.from(base64, "base64").byteLength;
}

function encodedChunk(request: VideoImportPortMessage): string {
  if (request.action !== "video_import_chunk") throw new Error("expected video chunk");
  return request.data_base64;
}

describe("browser video import", () => {
  it("streams a regular HTTP video into the local native import", async () => {
    const pageUrl = "https://example.com/work";
    const videoUrl = "https://example.com/media/demo.mp4";
    const bytes = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d, 0x01, 0x02, 0x03,
    ]);
    const fetchVideo = vi.fn(async () =>
      Promise.resolve(
        new Response(bytes, {
          headers: {
            "Content-Type": "video/mp4",
            "Content-Length": String(bytes.byteLength),
          },
        }),
      ),
    );
    const fetchPageVideo = vi.fn(async () => {
      throw new Error("the page bridge should not be needed for a readable HTTP video");
    });
    const recordVideoElement = vi.fn(async () => {
      throw new Error("the recorder should not be needed for a readable HTTP video");
    });
    const port = new AutoAckVideoImportPort();
    const doc = cosmosDocument(videoUrl);
    doc.head.insertAdjacentHTML("beforeend", '<meta name="theme-color" content="#123456">');

    await expect(
      importVideo({
        doc,
        pageUrl,
        mediaUrl: videoUrl,
        fetchVideo,
        fetchPageVideo,
        recordVideoElement,
        connect: () => port,
        createUploadId: () => "http-video-upload",
        savedAt: "2026-08-25T19:00:00.000Z",
      }),
    ).resolves.toMatchObject({ response: { ok: true, id: "saved-video-id" } });

    expect(fetchVideo).toHaveBeenCalledWith(videoUrl);
    expect(fetchPageVideo).not.toHaveBeenCalled();
    expect(recordVideoElement).not.toHaveBeenCalled();
    expect(port.messages.map((message) => message.action)).toEqual([
      "video_import_begin",
      "video_import_chunk",
      "video_import_finish",
    ]);
    expect(port.messages[0]).toMatchObject({
      action: "video_import_begin",
      expected_bytes: bytes.byteLength,
      metadata: { kind: "video", url: pageUrl, theme_color: "#123456" },
    });
  });

  it("continues in the page world when Dia returns an empty isolated-world response", async () => {
    const pageUrl = "https://www.cosmos.so/e/2035271300";
    const blobUrl = "blob:https://www.cosmos.so/e67ae631-bb86-4e81-a485-e93541e5d318";
    const bytes = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
    ]);
    const fetchBlob = vi.fn(async () =>
      Promise.resolve(
        new Response(new Uint8Array(), {
          headers: { "Content-Type": "video/mp4", "Content-Length": "0" },
        }),
      ),
    );
    const fetchPageBlob = vi.fn(async () =>
      Promise.resolve(
        new Response(bytes, {
          headers: {
            "Content-Type": "video/mp4",
            "Content-Length": String(bytes.byteLength),
          },
        }),
      ),
    );
    const recordVideoElement = vi.fn(async () => {
      throw new Error("the recorder should not be needed for a page-readable Blob");
    });
    const port = new AutoAckVideoImportPort();

    await expect(
      importBlobVideo({
        doc: cosmosDocument(blobUrl),
        pageUrl,
        blobUrl,
        fetchBlob,
        fetchPageBlob,
        recordVideoElement,
        connect: () => port,
        createUploadId: () => "cosmos-empty-dia-response",
        savedAt: "2026-08-25T18:00:00.000Z",
      }),
    ).resolves.toMatchObject({ response: { ok: true, id: "saved-video-id" } });

    expect(fetchBlob).toHaveBeenCalledWith(blobUrl);
    expect(fetchPageBlob).toHaveBeenCalledWith(blobUrl);
    expect(recordVideoElement).not.toHaveBeenCalled();
    expect(port.messages.map((message) => message.action)).toEqual([
      "video_import_begin",
      "video_import_chunk",
      "video_import_finish",
    ]);
  });

  it("records the rendered Cosmos video when its blob source says Failed to fetch", async () => {
    const pageUrl = "https://www.cosmos.so/e/2035271300";
    const blobUrl = "blob:https://www.cosmos.so/e67ae631-bb86-4e81-a485-e93541e5d318";
    const bytes = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
    ]);
    let read = false;
    const recordedResponse = {
      ok: true,
      headers: new Headers({
        "Content-Type": "video/mp4",
        "Content-Length": String(bytes.byteLength),
      }),
      body: {
        getReader: () => ({
          read: async () => {
            if (read) return { done: true as const, value: undefined };
            read = true;
            return { done: false as const, value: bytes };
          },
          releaseLock: () => undefined,
        }),
      },
    } as unknown as Response;
    const fetchBlob = vi.fn(async () => {
      throw new TypeError("Failed to fetch");
    });
    const fetchPageBlob = vi.fn(async () => {
      throw new TypeError("Failed to fetch");
    });
    const recordVideoElement = vi.fn(async () => recordedResponse);
    const port = new AutoAckVideoImportPort();

    await expect(
      importBlobVideo({
        doc: cosmosDocument(blobUrl),
        pageUrl,
        blobUrl,
        fetchBlob,
        fetchPageBlob,
        recordVideoElement,
        connect: () => port,
        createUploadId: () => "cosmos-page-world-upload",
        savedAt: "2026-08-25T17:30:00.000Z",
      }),
    ).resolves.toMatchObject({ response: { ok: true, id: "saved-video-id" } });

    expect(fetchBlob).toHaveBeenCalledWith(blobUrl);
    expect(fetchPageBlob).toHaveBeenCalledWith(blobUrl);
    expect(recordVideoElement).toHaveBeenCalledWith(expect.anything(), blobUrl);
    expect(port.messages.map((message) => message.action)).toEqual([
      "video_import_begin",
      "video_import_chunk",
      "video_import_finish",
    ]);
  });

  it("streams the exact Cosmos blob through one backpressured port without buffering it whole", async () => {
    const pageUrl = "https://www.cosmos.so/e/2035271300";
    const blobUrl = "blob:https://www.cosmos.so/3f7de4c2-d242-4cac-96dd-c04459f564b2";
    const first = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
    ]);
    const second = byteRange(MAX_VIDEO_IMPORT_CHUNK_BYTES + 7, first.length);
    const chunks = [first, second];
    let readIndex = 0;
    const read = vi.fn(async () => {
      const value = chunks[readIndex++];
      return value ? { done: false as const, value } : { done: true as const, value: undefined };
    });
    const arrayBuffer = vi.fn(() => {
      throw new Error("whole-video buffering is forbidden");
    });
    const response = {
      ok: true,
      headers: new Headers({
        "Content-Type": "video/mp4",
        "Content-Length": String(first.byteLength + second.byteLength),
      }),
      body: { getReader: () => ({ read, releaseLock: () => undefined }) },
      arrayBuffer,
    } as unknown as Response;
    const fetchBlob = vi.fn(async () => response);
    const port = new FakeVideoImportPort();
    const connect = vi.fn((name: string) => {
      expect(name).toBe(VIDEO_IMPORT_PORT_NAME);
      return port;
    });

    const imported = importBlobVideo({
      doc: cosmosDocument(blobUrl),
      pageUrl,
      blobUrl,
      fetchBlob,
      connect,
      createUploadId: () => "cosmos-upload",
      savedAt: "2026-08-25T15:00:00.000Z",
    });

    await vi.waitFor(() => expect(port.messages).toHaveLength(1));
    expect(port.messages[0]).toEqual({
      protocol_version: PROTOCOL_VERSION,
      action: "video_import_begin",
      upload_id: "cosmos-upload",
      metadata: {
        kind: "video",
        url: pageUrl,
        canonical_url: pageUrl,
        title: "Cosmos",
        site: "www.cosmos.so",
        saved_at: "2026-08-25T15:00:00.000Z",
      },
      content_type: "video/mp4",
      expected_bytes: first.byteLength + second.byteLength,
    });
    expect(read).toHaveBeenCalledTimes(1);

    port.acknowledge({ protocol_version: PROTOCOL_VERSION, ok: true });
    await vi.waitFor(() => expect(port.messages).toHaveLength(2));
    expect(port.messages[1]).toMatchObject({
      action: "video_import_chunk",
      upload_id: "cosmos-upload",
      sequence: 0,
    });
    expect(decodedLength(encodedChunk(port.messages[1]))).toBe(first.byteLength);
    expect(read).toHaveBeenCalledTimes(1);

    port.acknowledge({ protocol_version: PROTOCOL_VERSION, ok: true });
    await vi.waitFor(() => expect(port.messages).toHaveLength(3));
    expect(port.messages[2]).toMatchObject({ action: "video_import_chunk", sequence: 1 });
    expect(decodedLength(encodedChunk(port.messages[2]))).toBe(MAX_VIDEO_IMPORT_CHUNK_BYTES);
    expect(port.messages).toHaveLength(3);

    port.acknowledge({ protocol_version: PROTOCOL_VERSION, ok: true });
    await vi.waitFor(() => expect(port.messages).toHaveLength(4));
    expect(port.messages[3]).toMatchObject({ action: "video_import_chunk", sequence: 2 });
    expect(decodedLength(encodedChunk(port.messages[3]))).toBe(7);
    expect(port.messages).toHaveLength(4);

    port.acknowledge({ protocol_version: PROTOCOL_VERSION, ok: true });
    await vi.waitFor(() => expect(port.messages).toHaveLength(5));
    expect(port.messages[4]).toEqual({
      protocol_version: PROTOCOL_VERSION,
      action: "video_import_finish",
      upload_id: "cosmos-upload",
    });
    port.acknowledge({
      protocol_version: PROTOCOL_VERSION,
      ok: true,
      id: "saved-video-id",
      path: "articles/sa/saved-video-id/article.md",
    });

    await expect(imported).resolves.toMatchObject({
      metadata: { kind: "video", url: pageUrl, title: "Cosmos" },
      response: { ok: true, id: "saved-video-id" },
    });
    expect(fetchBlob).toHaveBeenCalledWith(blobUrl);
    expect(connect).toHaveBeenCalledTimes(1);
    expect(arrayBuffer).not.toHaveBeenCalled();
    expect(port.disconnected).toBe(true);
  });

  it.each([
    {
      name: "MP4",
      bytes: Uint8Array.from([
        0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
      ]),
      declared: "",
      expected: "video/mp4",
    },
    {
      name: "QuickTime",
      bytes: Uint8Array.from([
        0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x71, 0x74, 0x20, 0x20,
      ]),
      declared: "application/octet-stream",
      expected: "video/quicktime",
    },
    {
      name: "WebM",
      bytes: Uint8Array.from([0x1a, 0x45, 0xdf, 0xa3, 0x9f, 0x42, 0x86, 0x81]),
      declared: "binary/octet-stream",
      expected: "video/webm",
    },
  ])(
    "sniffs $name from the bounded first chunk when the blob MIME is blank or generic",
    async ({ bytes, declared, expected }) => {
      let read = false;
      const response = {
        ok: true,
        headers: new Headers(declared ? { "Content-Type": declared } : {}),
        body: {
          getReader: () => ({
            read: async () => {
              if (read) return { done: true as const, value: undefined };
              read = true;
              return { done: false as const, value: bytes };
            },
            releaseLock: () => undefined,
          }),
        },
      } as unknown as Response;
      const port = new AutoAckVideoImportPort();

      await importBlobVideo({
        doc: cosmosDocument("blob:https://www.cosmos.so/sniff"),
        pageUrl: "https://www.cosmos.so/e/2035271300",
        blobUrl: "blob:https://www.cosmos.so/sniff",
        fetchBlob: async () => response,
        connect: () => port,
        createUploadId: () => "sniff-upload",
      });

      expect(port.messages[0]).toMatchObject({
        action: "video_import_begin",
        content_type: expected,
      });
      expect(port.messages[0]).not.toHaveProperty("expected_bytes");
    },
  );

  it("returns a duplicate finish response so the worker can show Already in Óia", async () => {
    const bytes = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
    ]);
    let read = false;
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: async () => {
            if (read) return { done: true as const, value: undefined };
            read = true;
            return { done: false as const, value: bytes };
          },
          releaseLock: () => undefined,
        }),
      },
    } as unknown as Response;
    const port = new FakeVideoImportPort();
    const imported = importBlobVideo({
      doc: cosmosDocument("blob:https://www.cosmos.so/duplicate"),
      pageUrl: "https://www.cosmos.so/e/2035271300",
      blobUrl: "blob:https://www.cosmos.so/duplicate",
      fetchBlob: async () => response,
      connect: () => port,
      createUploadId: () => "duplicate-upload",
    });

    await vi.waitFor(() => expect(port.messages.at(-1)?.action).toBe("video_import_begin"));
    port.acknowledge({ protocol_version: PROTOCOL_VERSION, ok: true });
    await vi.waitFor(() => expect(port.messages.at(-1)?.action).toBe("video_import_chunk"));
    port.acknowledge({ protocol_version: PROTOCOL_VERSION, ok: true });
    await vi.waitFor(() => expect(port.messages.at(-1)?.action).toBe("video_import_finish"));
    port.acknowledge({
      protocol_version: PROTOCOL_VERSION,
      ok: false,
      error: "duplicate",
      message: "Already saved",
    });

    await expect(imported).resolves.toMatchObject({
      response: { ok: false, error: "duplicate" },
    });
    expect(port.messages.some((message) => message.action === "video_import_abort")).toBe(false);
  });

  it("sniffs an MP4 signature split across tiny initial stream chunks", async () => {
    const chunks = [
      Uint8Array.from([0x00, 0x00]),
      Uint8Array.from([0x00, 0x18, 0x66]),
      Uint8Array.from([0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d]),
    ];
    let readIndex = 0;
    const response = {
      ok: true,
      headers: new Headers(),
      body: {
        getReader: () => ({
          read: async () => {
            const value = chunks[readIndex++];
            return value
              ? { done: false as const, value }
              : { done: true as const, value: undefined };
          },
          releaseLock: () => undefined,
        }),
      },
    } as unknown as Response;
    const port = new AutoAckVideoImportPort();

    await importBlobVideo({
      doc: cosmosDocument("blob:https://www.cosmos.so/split-signature"),
      pageUrl: "https://www.cosmos.so/e/2035271300",
      blobUrl: "blob:https://www.cosmos.so/split-signature",
      fetchBlob: async () => response,
      connect: () => port,
      createUploadId: () => "split-signature-upload",
    });

    expect(port.messages[0]).toMatchObject({
      action: "video_import_begin",
      content_type: "video/mp4",
    });
    expect(port.messages.filter((message) => message.action === "video_import_chunk")).toHaveLength(
      3,
    );
  });

  it("retains native keepalive while preparing, then immediately cancels opening if the host fails", async () => {
    vi.useFakeTimers();
    const port = new FakeVideoImportPort();
    const connect = vi.fn(() => port);
    const fetchPageBlob = vi.fn();
    const imported = importBlobVideo({
      doc: cosmosDocument("blob:https://www.cosmos.so/early-native-error"),
      pageUrl: "https://www.cosmos.so/e/2035271300",
      blobUrl: "blob:https://www.cosmos.so/early-native-error",
      fetchBlob: () => new Promise(() => undefined),
      fetchPageBlob,
      connect,
      createUploadId: () => "early-error-upload",
    });
    const rejected = expect(imported).rejects.toMatchObject({
      code: "native_connection",
      message: "Specified native messaging host not found.",
    });
    expect(connect).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(VIDEO_PREPARATION_HEARTBEAT_MS);
    expect(port.messages.at(-1)?.action).toBe("video_import_prepare");
    port.acknowledge({
      protocol_version: PROTOCOL_VERSION,
      ok: false,
      error: "native_connection",
      message: "Specified native messaging host not found.",
    });
    port.drop();
    await rejected;
    expect(fetchPageBlob).not.toHaveBeenCalled();
    const sent = port.messages.length;
    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    expect(port.messages).toHaveLength(sent);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("closes its preparation connection when opening the source fails", async () => {
    const port = new FakeVideoImportPort();

    await expect(
      importBlobVideo({
        doc: cosmosDocument("blob:https://www.cosmos.so/fetch-failure"),
        pageUrl: "https://www.cosmos.so/e/2035271300",
        blobUrl: "blob:https://www.cosmos.so/fetch-failure",
        fetchBlob: async () => {
          throw new Error("blob fetch failed");
        },
        connect: () => port,
        createUploadId: () => "fetch-failure-upload",
      }),
    ).rejects.toThrow("blob fetch failed");

    expect(port.messages).toEqual([
      {
        protocol_version: PROTOCOL_VERSION,
        action: "video_import_abort",
        upload_id: "fetch-failure-upload",
      },
    ]);
    expect(port.disconnected).toBe(true);
  });

  it("immediately stops preparation after a native disconnect without a response", async () => {
    vi.useFakeTimers();
    const port = new FakeVideoImportPort();
    const mediaUrl = "https://example.com/stalled.mp4";
    const imported = importVideo({
      doc: cosmosDocument(mediaUrl),
      pageUrl: "https://example.com/work",
      mediaUrl,
      fetchVideo: () => new Promise(() => undefined),
      connect: () => port,
    });
    const rejected = expect(imported).rejects.toMatchObject({ code: "native_connection" });
    await vi.advanceTimersByTimeAsync(0);
    port.drop();
    await rejected;
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cancels a prepared reader when native disconnect wins before the upload adopts it", async () => {
    vi.useFakeTimers();
    const port = new FakeVideoImportPort();
    const cancel = vi.fn(async () => undefined);
    const releaseLock = vi.fn();
    const response = {
      ok: true,
      headers: {
        get(name: string) {
          if (name === "Content-Length") {
            // The probe has read valid bytes and is about to return its reader.
            port.drop();
            return null;
          }
          return "video/mp4";
        },
      },
      body: {
        getReader: () => ({
          read: async () => ({ done: false, value: Uint8Array.from([1]) }),
          cancel,
          releaseLock,
        }),
      },
    } as unknown as Response;
    const mediaUrl = "https://example.com/disconnect-at-ready.mp4";
    const imported = importVideo({
      doc: cosmosDocument(mediaUrl),
      pageUrl: "https://example.com/work",
      mediaUrl,
      fetchVideo: async () => response,
      connect: () => port,
    });
    const rejected = expect(imported).rejects.toMatchObject({ code: "native_connection" });

    await vi.advanceTimersByTimeAsync(0);
    await rejected;
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(releaseLock).toHaveBeenCalledTimes(1);
    expect(port.messages).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("stops its preparation heartbeat when posting to a disconnected port fails", async () => {
    vi.useFakeTimers();
    class DisconnectedPreparationPort extends FakeVideoImportPort {
      override postMessage(message: VideoImportPortMessage): void {
        if (message.action === "video_import_prepare") throw new Error("port disconnected");
        super.postMessage(message);
      }
    }
    const port = new DisconnectedPreparationPort();
    const mediaUrl = "https://example.com/stalled.mp4";
    const imported = importVideo({
      doc: cosmosDocument(mediaUrl),
      pageUrl: "https://example.com/work",
      mediaUrl,
      fetchVideo: () => new Promise(() => undefined),
      connect: () => port,
    });
    const rejected = expect(imported).rejects.toMatchObject({
      code: "native_connection",
      message: "port disconnected",
    });
    await vi.advanceTimersByTimeAsync(VIDEO_PREPARATION_HEARTBEAT_MS);
    await rejected;
    expect(port.disconnected).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cannot heartbeat indefinitely when every source-opening attempt stalls", async () => {
    vi.useFakeTimers();
    const mediaUrl = "https://example.com/stalled.mp4";
    const port = new AutoAckVideoImportPort();
    const stalled = () => new Promise<Response>(() => undefined);
    const imported = importVideo({
      doc: cosmosDocument(mediaUrl),
      pageUrl: "https://example.com/work",
      mediaUrl,
      fetchVideo: stalled,
      fetchPageVideo: stalled,
      recordVideoElement: stalled,
      connect: () => port,
    });
    const rejected = expect(imported).rejects.toMatchObject({ code: "video_timeout" });
    await vi.advanceTimersByTimeAsync(VIDEO_PREPARATION_TIMEOUT_MS);
    await rejected;
    expect(port.disconnected).toBe(true);
    expect(port.messages.some((message) => message.action === "video_import_prepare")).toBe(true);
    expect(port.messages.some((message) => message.action === "video_import_begin")).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("tries the page source after the initial fetch stops responding", async () => {
    vi.useFakeTimers();
    const mediaUrl = "https://example.com/stalled.mp4";
    const port = new AutoAckVideoImportPort();
    const fetchPageVideo = vi.fn(
      async () =>
        new Response(Uint8Array.from([1]), {
          headers: { "Content-Type": "video/mp4" },
        }),
    );
    const imported = importVideo({
      doc: cosmosDocument(mediaUrl),
      pageUrl: "https://example.com/work",
      mediaUrl,
      fetchVideo: () => new Promise(() => undefined),
      fetchPageVideo,
      connect: () => port,
    });

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    await expect(imported).resolves.toMatchObject({ response: { ok: true } });
    expect(fetchPageVideo).toHaveBeenCalledWith(mediaUrl);
    expect(port.disconnected).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each(["video_import_begin", "video_import_chunk", "video_import_finish"] as const)(
    "closes the native port and cancels its source when %s never acknowledges",
    async (stalledAction) => {
      vi.useFakeTimers();
      const mediaUrl = "https://example.com/no-ack.mp4";
      class StalledAckPort extends FakeVideoImportPort {
        override postMessage(message: VideoImportPortMessage): void {
          super.postMessage(message);
          if (message.action !== stalledAction) {
            queueMicrotask(() =>
              this.acknowledge({ protocol_version: PROTOCOL_VERSION, ok: true }),
            );
          }
        }
      }
      const port = new StalledAckPort();
      let first = true;
      const cancel = vi.fn(async () => undefined);
      const releaseLock = vi.fn();
      const response = {
        ok: true,
        headers: new Headers({ "Content-Type": "video/mp4" }),
        body: {
          getReader: () => ({
            read: async () => {
              if (!first) return { done: true, value: undefined };
              first = false;
              return { done: false, value: Uint8Array.from([1]) };
            },
            cancel,
            releaseLock,
          }),
        },
      } as unknown as Response;
      const imported = importVideo({
        doc: cosmosDocument(mediaUrl),
        pageUrl: "https://example.com/work",
        mediaUrl,
        fetchVideo: async () => response,
        connect: () => port,
      });
      const rejected = expect(imported).rejects.toMatchObject({ code: "video_timeout" });
      await vi.advanceTimersByTimeAsync(0);
      expect(port.messages.at(-1)?.action).toBe(stalledAction);

      if (stalledAction === "video_import_finish") {
        await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
        expect(port.disconnected).toBe(false);
        await vi.advanceTimersByTimeAsync(
          VIDEO_TRANSFER_FINISH_TIMEOUT_MS - VIDEO_TRANSFER_TIMEOUT_MS,
        );
      } else {
        await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
      }
      await rejected;
      expect(port.disconnected).toBe(true);
      expect(cancel).toHaveBeenCalledTimes(1);
      expect(releaseLock).toHaveBeenCalled();
      expect(vi.getTimerCount()).toBe(0);
    },
  );

  it("closes its port before a stalled stream's cancellation can hang cleanup", async () => {
    vi.useFakeTimers();
    const mediaUrl = "https://example.com/stalled-body.mp4";
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
    const port = new AutoAckVideoImportPort();
    const imported = importVideo({
      doc: cosmosDocument(mediaUrl),
      pageUrl: "https://example.com/work",
      mediaUrl,
      fetchVideo: async () => response,
      connect: () => port,
    });
    const rejected = expect(imported).rejects.toMatchObject({ code: "video_timeout" });

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    expect(port.disconnected).toBe(true);
    expect(port.messages.at(-1)?.action).toBe("video_import_abort");
    expect(cancel).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS);
    await rejected;
    expect(releaseLock).toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("allows a slow stream to continue for longer than the inactivity deadline", async () => {
    vi.useFakeTimers();
    const mediaUrl = "https://example.com/progressing.mp4";
    let reads = 0;
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: () =>
            new Promise((resolve) =>
              setTimeout(() => {
                reads += 1;
                resolve(
                  reads <= 3 ? { done: false, value: Uint8Array.from([reads]) } : { done: true },
                );
              }, 40_000),
            ),
          releaseLock: vi.fn(),
        }),
      },
    } as unknown as Response;
    const port = new AutoAckVideoImportPort();
    const imported = importVideo({
      doc: cosmosDocument(mediaUrl),
      pageUrl: "https://example.com/work",
      mediaUrl,
      fetchVideo: async () => response,
      connect: () => port,
    });

    await vi.advanceTimersByTimeAsync(160_000);
    await expect(imported).resolves.toMatchObject({ response: { ok: true } });
    expect(port.messages.filter((message) => message.action === "video_import_chunk")).toHaveLength(
      3,
    );
    expect(vi.getTimerCount()).toBe(0);
  });

  it("aborts after a Response.body reader failure", async () => {
    const first = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
    ]);
    let readCount = 0;
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: async () => {
            readCount += 1;
            if (readCount === 1) return { done: false as const, value: first };
            throw new Error("video stream read failed");
          },
          releaseLock: () => undefined,
        }),
      },
    } as unknown as Response;
    const port = new AutoAckVideoImportPort();

    await expect(
      importBlobVideo({
        doc: cosmosDocument("blob:https://www.cosmos.so/read-failure"),
        pageUrl: "https://www.cosmos.so/e/2035271300",
        blobUrl: "blob:https://www.cosmos.so/read-failure",
        fetchBlob: async () => response,
        connect: () => port,
        createUploadId: () => "read-failure-upload",
      }),
    ).rejects.toThrow("video stream read failed");

    expect(port.messages.map((message) => message.action)).toEqual([
      "video_import_begin",
      "video_import_chunk",
      "video_import_abort",
    ]);
  });

  it("aborts after a native chunk acknowledgement fails", async () => {
    const first = Uint8Array.from([
      0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d,
    ]);
    let read = false;
    const response = {
      ok: true,
      headers: new Headers({ "Content-Type": "video/mp4" }),
      body: {
        getReader: () => ({
          read: async () => {
            if (read) return { done: true as const, value: undefined };
            read = true;
            return { done: false as const, value: first };
          },
          releaseLock: () => undefined,
        }),
      },
    } as unknown as Response;
    const port = new FakeVideoImportPort();
    const imported = importBlobVideo({
      doc: cosmosDocument("blob:https://www.cosmos.so/native-failure"),
      pageUrl: "https://www.cosmos.so/e/2035271300",
      blobUrl: "blob:https://www.cosmos.so/native-failure",
      fetchBlob: async () => response,
      connect: () => port,
      createUploadId: () => "native-failure-upload",
    });

    await vi.waitFor(() => expect(port.messages.at(-1)?.action).toBe("video_import_begin"));
    port.acknowledge({ protocol_version: PROTOCOL_VERSION, ok: true });
    await vi.waitFor(() => expect(port.messages.at(-1)?.action).toBe("video_import_chunk"));
    port.acknowledge({
      protocol_version: PROTOCOL_VERSION,
      ok: false,
      error: "write_failed",
      message: "native chunk write failed",
    });

    await expect(imported).rejects.toMatchObject({
      name: "VideoImportError",
      code: "write_failed",
      message: "native chunk write failed",
    });
    expect(port.messages.map((message) => message.action)).toEqual([
      "video_import_begin",
      "video_import_chunk",
      "video_import_abort",
    ]);
  });
});
