// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  findVideoElement,
  recordVideoElement,
  selectMp4RecorderMimeType,
} from "./video-element-recorder.js";
import { VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS, VIDEO_TRANSFER_TIMEOUT_MS } from "./video-transfer.js";

afterEach(() => vi.useRealTimers());

class FakeTrack extends EventTarget {
  muted = false;
  readonly stop = vi.fn();

  unmute(): void {
    this.muted = false;
    this.dispatchEvent(new Event("unmute"));
  }
}

class FakeMediaRecorder extends EventTarget {
  static supported = new Set<string>();
  static instance: FakeMediaRecorder | undefined;

  static isTypeSupported(mimeType: string): boolean {
    return this.supported.has(mimeType);
  }

  state: RecordingState = "inactive";
  readonly mimeType: string;
  readonly start = vi.fn((_: number) => {
    this.state = "recording";
  });
  readonly pause = vi.fn(() => {
    this.state = "paused";
  });
  readonly resume = vi.fn(() => {
    this.state = "recording";
  });
  finalBlob = new Blob();

  constructor(_stream: MediaStream, options?: MediaRecorderOptions) {
    super();
    this.mimeType = options?.mimeType ?? "";
    FakeMediaRecorder.instance = this;
  }

  stop(): void {
    this.state = "inactive";
    this.emitData(this.finalBlob);
    this.dispatchEvent(new Event("stop"));
  }

  emitData(blob: Blob): void {
    const event = new Event("dataavailable") as BlobEvent;
    Object.defineProperty(event, "data", { value: blob });
    this.dispatchEvent(event);
  }
}

function cosmosVideo(blobUrl: string): {
  doc: Document;
  video: HTMLVideoElement;
  videoTrack: FakeTrack;
  captureStream: ReturnType<typeof vi.fn>;
  setCurrentTime(value: number): void;
  getState(): { paused: boolean; currentTime: number };
} {
  const doc = document.implementation.createHTMLDocument();
  doc.body.innerHTML = `<video data-testid="element-view-video" loop src="${blobUrl}"></video>`;
  const video = doc.querySelector("video")!;
  const videoTrack = new FakeTrack();
  let paused = true;
  let currentTime = 2;
  Object.defineProperties(video, {
    currentSrc: { configurable: true, get: () => blobUrl },
    duration: { configurable: true, get: () => 4 },
    paused: { configurable: true, get: () => paused },
    currentTime: {
      configurable: true,
      get: () => currentTime,
      set: (value: number) => {
        currentTime = value;
      },
    },
  });
  video.loop = false;
  video.muted = false;
  video.playbackRate = 1.5;
  video.play = vi.fn(async () => {
    paused = false;
  });
  video.pause = vi.fn(() => {
    paused = true;
  });
  const mediaStream = {
    getTracks: () => [videoTrack],
    getVideoTracks: () => [videoTrack],
    getAudioTracks: () => [],
  } as unknown as MediaStream;
  const captureStream = vi.fn(() => mediaStream);
  Object.defineProperty(video, "captureStream", { value: captureStream });
  return {
    doc,
    video,
    videoTrack,
    captureStream,
    setCurrentTime(value) {
      currentTime = value;
      video.dispatchEvent(new Event("timeupdate"));
    },
    getState: () => ({ paused, currentTime }),
  };
}

describe("rendered video recorder", () => {
  it("locates the exact Cosmos video instead of a neighboring video", () => {
    const blobUrl = "blob:https://www.cosmos.so/e67ae631-bb86-4e81-a485-e93541e5d318";
    const doc = document.implementation.createHTMLDocument();
    doc.body.innerHTML = `<video src="blob:https://www.cosmos.so/other"></video><video data-testid="element-view-video" src="${blobUrl}"></video>`;

    expect(findVideoElement(doc, blobUrl)?.dataset.testid).toBe("element-view-video");
  });

  it("selects explicit H.264 MP4 and never falls back to bare MP4 or WebM", () => {
    const supported = new Set([
      'video/mp4;codecs="avc1.42E01F,mp4a.40.2"',
      'video/mp4;codecs="avc1.42E01F"',
      "video/mp4",
      "video/webm;codecs=vp9",
    ]);
    const Recorder = { isTypeSupported: (mime: string) => supported.has(mime) };

    expect(selectMp4RecorderMimeType(Recorder, true)).toBe(
      'video/mp4;codecs="avc1.42E01F,mp4a.40.2"',
    );
    expect(selectMp4RecorderMimeType(Recorder, false)).toBe('video/mp4;codecs="avc1.42E01F"');
    expect(
      selectMp4RecorderMimeType(
        { isTypeSupported: (mime) => mime === "video/mp4" || mime.startsWith("video/webm") },
        false,
      ),
    ).toBeUndefined();
  });

  it("streams one complete loop with backpressure and restores the page video", async () => {
    const blobUrl = "blob:https://www.cosmos.so/e67ae631-bb86-4e81-a485-e93541e5d318";
    const fixture = cosmosVideo(blobUrl);
    const mimeType = 'video/mp4;codecs="avc1.42E01F"';
    FakeMediaRecorder.supported = new Set([mimeType]);
    const timers: Array<() => void> = [];

    const response = await recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
      setTimeout: ((callback: TimerHandler) => {
        timers.push(callback as () => void);
        return 1 as unknown as ReturnType<typeof setTimeout>;
      }) as unknown as typeof setTimeout,
      clearTimeout: vi.fn() as unknown as typeof clearTimeout,
    });
    const recorder = FakeMediaRecorder.instance!;
    const reader = response.body!.getReader();
    const firstBytes = Uint8Array.from([0, 0, 0, 24, 102, 116, 121, 112]);
    const finalBytes = Uint8Array.from([9, 8, 7]);
    recorder.finalBlob = new Blob([finalBytes], { type: mimeType });

    const firstRead = reader.read();
    recorder.emitData(new Blob([firstBytes], { type: mimeType }));
    const first = await firstRead;
    expect(first.done).toBe(false);
    expect(Array.from(first.value ?? [])).toEqual(Array.from(firstBytes));
    expect(recorder.pause).toHaveBeenCalledTimes(1);
    expect(fixture.getState().paused).toBe(true);

    const secondRead = reader.read();
    await vi.waitFor(() => expect(recorder.resume).toHaveBeenCalledTimes(1));
    fixture.setCurrentTime(3);
    fixture.setCurrentTime(1);
    fixture.setCurrentTime(2);
    const second = await secondRead;
    expect(second.done).toBe(false);
    expect(Array.from(second.value ?? [])).toEqual(Array.from(finalBytes));
    await expect(reader.read()).resolves.toEqual({ done: true, value: undefined });

    expect(response.headers.get("Content-Type")).toBe(mimeType);
    expect(fixture.video.loop).toBe(false);
    expect(fixture.video.muted).toBe(false);
    expect(fixture.video.playbackRate).toBe(1.5);
    expect(fixture.getState()).toEqual({ paused: true, currentTime: 2 });
    expect(fixture.videoTrack.stop).toHaveBeenCalledTimes(1);
    expect(timers).toHaveLength(1);
  });

  it("rejects a recorder that stops after one chunk before a complete video loop", async () => {
    const blobUrl = "blob:https://example.com/premature-stop";
    const fixture = cosmosVideo(blobUrl);
    const mimeType = 'video/mp4;codecs="avc1.42E01F"';
    FakeMediaRecorder.supported = new Set([mimeType]);

    const response = await recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
      setTimeout: vi.fn(() => 1) as unknown as typeof setTimeout,
      clearTimeout: vi.fn() as unknown as typeof clearTimeout,
    });
    const recorder = FakeMediaRecorder.instance!;
    const reader = response.body!.getReader();
    const firstRead = reader.read();
    recorder.emitData(
      new Blob([Uint8Array.from([0, 0, 0, 24, 102, 116, 121, 112])], { type: mimeType }),
    );
    await expect(firstRead).resolves.toMatchObject({ done: false });

    recorder.state = "inactive";
    recorder.dispatchEvent(new Event("stop"));

    await expect(reader.read()).rejects.toThrow(
      "The temporary video stopped before a complete copy was recorded.",
    );
  });

  it("starts recording only after the rendered video starts playing", async () => {
    const blobUrl = "blob:https://www.cosmos.so/paused-media-source";
    const fixture = cosmosVideo(blobUrl);
    const mimeType = 'video/mp4;codecs="avc1.42E01F"';
    FakeMediaRecorder.supported = new Set([mimeType]);
    FakeMediaRecorder.instance = undefined;
    let resolvePlayback: (() => void) | undefined;
    fixture.video.play = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolvePlayback = resolve;
        }),
    );

    const responsePromise = recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
      setTimeout: vi.fn(() => 1) as unknown as typeof setTimeout,
      clearTimeout: vi.fn() as unknown as typeof clearTimeout,
    });

    await vi.waitFor(() => expect(fixture.video.play).toHaveBeenCalledTimes(1));
    expect(fixture.captureStream).not.toHaveBeenCalled();
    expect(FakeMediaRecorder.instance).toBeUndefined();

    resolvePlayback?.();
    const response = await responsePromise;
    const recorder = FakeMediaRecorder.instance!;
    expect(recorder.start).toHaveBeenCalledWith(1_000);

    recorder.finalBlob = new Blob([Uint8Array.from([1])], { type: mimeType });
    recorder.stop();
    await response.body?.cancel();
  });

  it("waits for the captured video track to produce frames", async () => {
    const blobUrl = "blob:https://www.cosmos.so/muted-media-source";
    const fixture = cosmosVideo(blobUrl);
    const mimeType = 'video/mp4;codecs="avc1.42E01F"';
    FakeMediaRecorder.supported = new Set([mimeType]);
    FakeMediaRecorder.instance = undefined;
    fixture.videoTrack.muted = true;

    const responsePromise = recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
      setTimeout: vi.fn(() => 1) as unknown as typeof setTimeout,
      clearTimeout: vi.fn() as unknown as typeof clearTimeout,
    });

    await vi.waitFor(() => expect(fixture.captureStream).toHaveBeenCalledTimes(1));
    expect(FakeMediaRecorder.instance).toBeUndefined();

    fixture.videoTrack.unmute();
    const response = await responsePromise;
    const recorder = FakeMediaRecorder.instance!;
    expect(recorder.start).toHaveBeenCalledWith(1_000);

    recorder.finalBlob = new Blob([Uint8Array.from([1])], { type: mimeType });
    recorder.stop();
    await response.body?.cancel();
  });

  it("fails clearly when the captured video track stays muted", async () => {
    const blobUrl = "blob:https://www.cosmos.so/unrecordable-media-source";
    const fixture = cosmosVideo(blobUrl);
    const mimeType = 'video/mp4;codecs="avc1.42E01F"';
    FakeMediaRecorder.supported = new Set([mimeType]);
    FakeMediaRecorder.instance = undefined;
    fixture.videoTrack.muted = true;
    const timers: Array<() => void> = [];

    const responsePromise = recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
      setTimeout: ((callback: TimerHandler) => {
        timers.push(callback as () => void);
        return 1 as unknown as ReturnType<typeof setTimeout>;
      }) as unknown as typeof setTimeout,
      clearTimeout: vi.fn() as unknown as typeof clearTimeout,
    });

    await vi.waitFor(() => expect(timers).toHaveLength(1));
    timers[0]();

    await expect(responsePromise).rejects.toThrow(
      "The temporary video did not start producing recordable frames.",
    );
    expect(FakeMediaRecorder.instance).toBeUndefined();
    expect(fixture.videoTrack.stop).toHaveBeenCalledTimes(1);
    expect(fixture.video.loop).toBe(false);
    expect(fixture.video.muted).toBe(false);
    expect(fixture.video.playbackRate).toBe(1.5);
  });

  it("fails clearly when the browser can only record an unplayable format", async () => {
    const blobUrl = "blob:https://www.cosmos.so/webm-only";
    const fixture = cosmosVideo(blobUrl);
    FakeMediaRecorder.supported = new Set(["video/webm;codecs=vp9"]);

    await expect(
      recordVideoElement(fixture.doc, blobUrl, {
        MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
      }),
    ).rejects.toThrow("Óia-compatible MP4");
    expect(fixture.videoTrack.stop).toHaveBeenCalledTimes(1);
  });

  it("restores playback state when cancelled while play never resolves", async () => {
    const blobUrl = "blob:https://example.com/stalled-play";
    const fixture = cosmosVideo(blobUrl);
    fixture.video.play = vi.fn(() => new Promise<void>(() => undefined));
    const cancellation = new AbortController();
    const recording = recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
      signal: cancellation.signal,
    });
    const rejected = expect(recording).rejects.toThrow("capture cancelled");
    cancellation.abort(new Error("capture cancelled"));
    await rejected;

    expect(fixture.captureStream).not.toHaveBeenCalled();
    expect(fixture.getState()).toEqual({ paused: true, currentTime: 2 });
    expect(fixture.video.loop).toBe(false);
    expect(fixture.video.muted).toBe(false);
    expect(fixture.video.playbackRate).toBe(1.5);
  });

  it("times out stalled playback setup and restores its snapshot", async () => {
    vi.useFakeTimers();
    const blobUrl = "blob:https://example.com/stalled-play";
    const fixture = cosmosVideo(blobUrl);
    fixture.video.play = vi.fn(() => new Promise<void>(() => undefined));
    const recording = recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
    });
    const rejected = expect(recording).rejects.toThrow("did not start playing");

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    await rejected;
    expect(fixture.getState()).toEqual({ paused: true, currentTime: 2 });
    expect(fixture.video.loop).toBe(false);
    expect(fixture.video.muted).toBe(false);
    expect(fixture.video.playbackRate).toBe(1.5);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cancels track preparation without retaining its readiness timer", async () => {
    vi.useFakeTimers();
    const blobUrl = "blob:https://example.com/stalled-track";
    const fixture = cosmosVideo(blobUrl);
    fixture.videoTrack.muted = true;
    const cancellation = new AbortController();
    const recording = recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
      signal: cancellation.signal,
    });
    const rejected = expect(recording).rejects.toThrow("capture cancelled");
    await vi.advanceTimersByTimeAsync(0);
    expect(fixture.captureStream).toHaveBeenCalledTimes(1);
    cancellation.abort(new Error("capture cancelled"));
    await rejected;

    expect(fixture.videoTrack.stop).toHaveBeenCalledTimes(1);
    expect(fixture.getState()).toEqual({ paused: true, currentTime: 2 });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("stops and restores an active recorder on source cancellation", async () => {
    vi.useFakeTimers();
    const blobUrl = "blob:https://example.com/active-recording";
    const fixture = cosmosVideo(blobUrl);
    FakeMediaRecorder.supported = new Set(['video/mp4;codecs="avc1.42E01F"']);
    const cancellation = new AbortController();
    const response = await recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
      signal: cancellation.signal,
    });
    const reader = response.body!.getReader();
    const rejected = expect(reader.read()).rejects.toThrow("capture cancelled");
    cancellation.abort(new Error("capture cancelled"));
    await rejected;

    expect(FakeMediaRecorder.instance?.state).toBe("inactive");
    expect(fixture.videoTrack.stop).toHaveBeenCalledTimes(1);
    expect(fixture.getState()).toEqual({ paused: true, currentTime: 2 });
    reader.releaseLock();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("restores playback before waiting for a blob reader whose cancellation hangs", async () => {
    vi.useFakeTimers();
    const blobUrl = "blob:https://example.com/stalled-blob-cancel";
    const fixture = cosmosVideo(blobUrl);
    FakeMediaRecorder.supported = new Set(['video/mp4;codecs="avc1.42E01F"']);
    const response = await recordVideoElement(fixture.doc, blobUrl, {
      MediaRecorder: FakeMediaRecorder as unknown as typeof MediaRecorder,
    });
    const reader = response.body!.getReader();
    const pending = reader.read();
    const cancel = vi.fn(() => new Promise<void>(() => undefined));
    const releaseLock = vi.fn();
    FakeMediaRecorder.instance!.emitData({
      size: 1,
      stream: () => ({
        getReader: () => ({ read: () => new Promise(() => undefined), cancel, releaseLock }),
      }),
    } as unknown as Blob);
    await vi.advanceTimersByTimeAsync(0);
    const cancelled = reader.cancel();
    await vi.advanceTimersByTimeAsync(0);

    expect(fixture.videoTrack.stop).toHaveBeenCalledTimes(1);
    expect(fixture.getState()).toEqual({ paused: true, currentTime: 2 });
    expect(cancel).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS);
    await cancelled;
    await expect(pending).resolves.toMatchObject({ done: true });
    expect(releaseLock).toHaveBeenCalledTimes(1);
    reader.releaseLock();
    expect(vi.getTimerCount()).toBe(0);
  });
});
