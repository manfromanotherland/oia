// SPDX-License-Identifier: MIT

import { beforeEach, describe, expect, it, vi } from "vitest";

import { bytesToBase64, capTotalBytes, fetchImage, fetchImages } from "./images.js";

function mockResponse(
  bytes: number[],
  contentType = "image/png",
  ok = true,
  contentLength?: number,
  responseUrl = "",
) {
  return {
    ok,
    url: responseUrl,
    headers: {
      get: (key: string) => {
        if (key.toLowerCase() === "content-type") return contentType;
        if (key.toLowerCase() === "content-length" && contentLength !== undefined) {
          return String(contentLength);
        }
        return null;
      },
    },
    body: new ReadableStream<Uint8Array>({
      start(controller) {
        if (bytes.length) controller.enqueue(new Uint8Array(bytes));
        controller.close();
      },
    }),
  } as unknown as Response;
}

describe("bytesToBase64", () => {
  it("encodes bytes to standard base64", () => {
    expect(bytesToBase64(new Uint8Array([104, 105]))).toBe("aGk="); // "hi"
  });

  it("returns empty string for empty input", () => {
    expect(bytesToBase64(new Uint8Array([]))).toBe("");
  });
});

describe("fetchImage", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("returns url, trimmed content type, and base64 bytes on success", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(mockResponse([104, 105], "image/png; charset=binary")),
    );
    expect(await fetchImage("https://e.com/a.png")).toEqual({
      url: "https://e.com/a.png",
      content_type: "image/png",
      data_base64: "aGk=",
    });
  });

  it("returns null on a non-ok response", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(mockResponse([1], "image/png", false)));
    expect(await fetchImage("https://e.com/a.png")).toBeNull();
  });

  it("returns null when the fetch throws (CORS block, abort, network)", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    expect(await fetchImage("https://e.com/a.png")).toBeNull();
  });

  it("returns null for an empty body", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(mockResponse([], "image/png")));
    expect(await fetchImage("https://e.com/a.png")).toBeNull();
  });

  it("rejects oversized and non-image responses before encoding", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(mockResponse([1], "image/png", true, 41 * 1024 * 1024)),
    );
    expect(await fetchImage("https://e.com/huge.png")).toBeNull();

    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(mockResponse([1], "text/html")));
    expect(await fetchImage("https://e.com/not-an-image.png")).toBeNull();
  });

  it("allows page-local image responses but rejects unsafe HTTP redirects", async () => {
    const blobUrl = "blob:https://e.com/page-image";
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(mockResponse([1, 2], "image/png", true, 2, blobUrl)),
    );
    expect(await fetchImage(blobUrl)).toMatchObject({ url: blobUrl, data_base64: "AQI=" });

    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(mockResponse([1], "image/png", true, 1, "data:image/png,A")),
    );
    expect(await fetchImage("https://e.com/redirect.png")).toBeNull();
  });
});

describe("fetchImages", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("dedupes URLs and splits readable from unresolved", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockImplementation((url: string) =>
          url.includes("bad")
            ? Promise.reject(new TypeError("cors"))
            : Promise.resolve(mockResponse([104, 105], "image/png")),
        ),
    );
    const { images, unresolved } = await fetchImages([
      "https://e.com/a.png",
      "https://e.com/a.png", // duplicate — fetched once
      "https://e.com/bad.png",
    ]);
    expect(images.map((i) => i.url)).toEqual(["https://e.com/a.png"]);
    expect(unresolved).toEqual(["https://e.com/bad.png"]);
  });

  it("preserves candidate order when concurrent fetches finish out of order", async () => {
    let resolveFirst!: (value: ReturnType<typeof mockResponse>) => void;
    vi.stubGlobal(
      "fetch",
      vi.fn().mockImplementation((url: string) => {
        if (url.endsWith("first.png")) {
          return new Promise((resolve) => {
            resolveFirst = resolve;
          });
        }
        return Promise.resolve(mockResponse([2]));
      }),
    );

    const pending = fetchImages(["https://e.com/first.png", "https://e.com/second.png"]);
    await Promise.resolve();
    resolveFirst(mockResponse([1]));

    expect((await pending).images.map((image) => image.url)).toEqual([
      "https://e.com/first.png",
      "https://e.com/second.png",
    ]);
  });

  it("fits against the remaining budget before encoding and does not retry budget drops", async () => {
    const encode = vi.spyOn(globalThis, "btoa");
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockImplementation((url: string) =>
          Promise.resolve(mockResponse(url.endsWith("small.png") ? [5, 6] : [1, 2, 3, 4])),
        ),
    );
    const result = await fetchImages(
      ["https://e.com/first.png", "https://e.com/over-budget.png", "https://e.com/small.png"],
      { maxTotalBytes: 6 },
    );
    expect(result.images.map((value) => value.url)).toEqual([
      "https://e.com/first.png",
      "https://e.com/small.png",
    ]);
    expect(result.skipped).toEqual(["https://e.com/over-budget.png"]);
    expect(result.unresolved).toEqual([]);
    expect(encode).toHaveBeenCalledTimes(2);
  });

  it("stops enqueuing URLs and aborts bounded prefetch when the total budget fills", async () => {
    const requests: Array<{ url: string; signal: AbortSignal }> = [];
    vi.stubGlobal(
      "fetch",
      vi.fn().mockImplementation((url: string, options: RequestInit) => {
        requests.push({ url, signal: options.signal as AbortSignal });
        return Promise.resolve(mockResponse([1, 2, 3]));
      }),
    );
    const urls = Array.from({ length: 20 }, (_, index) => `https://e.com/${index}.png`);
    const result = await fetchImages(urls, { maxTotalBytes: 3, concurrency: 2 });
    expect(requests).toHaveLength(2);
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
    expect(result.images.map((value) => value.url)).toEqual([urls[0]]);
    expect(result.skipped).toEqual(urls.slice(1));
    expect(result.unresolved).toEqual([]);
  });

  it("cancels a prefetched response that settles after the page budget fills", async () => {
    let resolveLater!: (response: Response) => void;
    const cancelBody = vi.fn();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockImplementation((url: string) =>
        url.endsWith("later.png")
          ? new Promise((resolve) => {
              resolveLater = resolve;
            })
          : Promise.resolve(mockResponse([1, 2, 3])),
      ),
    );
    const result = await fetchImages(["https://e.com/first.png", "https://e.com/later.png"], {
      maxTotalBytes: 3,
    });
    expect(result.skipped).toEqual(["https://e.com/later.png"]);
    resolveLater({
      ...mockResponse([]),
      body: new ReadableStream<Uint8Array>({ cancel: cancelBody }),
    } as Response);
    await Promise.resolve();
    expect(cancelBody).toHaveBeenCalledTimes(1);
  });

  it("keeps slow earlier candidates ahead of faster later candidates for a small budget", async () => {
    let resolveFirst!: (value: ReturnType<typeof mockResponse>) => void;
    vi.stubGlobal(
      "fetch",
      vi.fn().mockImplementation((url: string) => {
        if (url.endsWith("first.png"))
          return new Promise((resolve) => {
            resolveFirst = resolve;
          });
        return Promise.resolve(mockResponse([2, 2, 2]));
      }),
    );
    const pending = fetchImages(["https://e.com/first.png", "https://e.com/second.png"], {
      maxTotalBytes: 3,
      concurrency: 2,
    });
    await Promise.resolve();
    resolveFirst(mockResponse([1, 1, 1]));
    const result = await pending;
    expect(result.images.map((value) => value.url)).toEqual(["https://e.com/first.png"]);
    expect(result.skipped).toEqual(["https://e.com/second.png"]);
  });

  it("marks per-image byte overflow as skipped but network failures as unresolved", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockImplementation((url: string) =>
          url.endsWith("failed.png")
            ? Promise.reject(new TypeError("CORS"))
            : Promise.resolve(mockResponse([1, 2, 3], "image/png", true, 3)),
        ),
    );
    const result = await fetchImages(["https://e.com/large.png", "https://e.com/failed.png"], {
      maxImageBytes: 2,
      maxTotalBytes: 10,
    });
    expect(result.images).toEqual([]);
    expect(result.skipped).toEqual(["https://e.com/large.png"]);
    expect(result.unresolved).toEqual(["https://e.com/failed.png"]);
  });

  it("does not fetch anything for a zero byte budget", async () => {
    const fetch = vi.fn();
    vi.stubGlobal("fetch", fetch);
    const result = await fetchImages(["https://e.com/a.png"], { maxTotalBytes: 0 });
    expect(fetch).not.toHaveBeenCalled();
    expect(result).toEqual({ images: [], unresolved: [], skipped: ["https://e.com/a.png"] });
  });
});

describe("capTotalBytes", () => {
  const img = (url: string, base64Len: number) => ({
    url,
    content_type: "image/png",
    data_base64: "A".repeat(base64Len),
  });

  it("keeps images in order until the byte budget would be exceeded", () => {
    // base64 length 8 → ~6 decoded bytes each; budget 12 fits two.
    const kept = capTotalBytes([img("a", 8), img("b", 8), img("c", 8)], 12);
    expect(kept.map((i) => i.url)).toEqual(["a", "b"]);
  });

  it("keeps everything when under budget", () => {
    expect(capTotalBytes([img("a", 4)], 1000)).toHaveLength(1);
  });
});
