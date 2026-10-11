// SPDX-License-Identifier: MIT

import {
  cancelVideoReader,
  VIDEO_TRANSFER_TIMEOUT_MS,
  waitForVideoTransfer,
} from "./video-transfer.js";

const RECORDING_SLICE_MS = 1_000;
const MAX_RECORDING_WALL_TIME_MS = 24 * 60 * 60 * 1_000;
const VIDEO_TRACK_READY_TIMEOUT_MS = 3_000;

type CapturableVideo = HTMLVideoElement & {
  captureStream?: () => MediaStream;
  mozCaptureStream?: () => MediaStream;
};

interface MediaRecorderConstructor {
  new (stream: MediaStream, options?: MediaRecorderOptions): MediaRecorder;
  isTypeSupported(mimeType: string): boolean;
}

export interface VideoElementRecorderEnvironment {
  MediaRecorder?: MediaRecorderConstructor;
  setTimeout?: typeof setTimeout;
  clearTimeout?: typeof clearTimeout;
  signal?: AbortSignal;
}

interface VideoSnapshot {
  loop: boolean;
  muted: boolean;
  paused: boolean;
  currentTime: number;
  playbackRate: number;
}

/**
 * Record one complete loop of the exact rendered video when its object URL is
 * backed by MediaSource and therefore cannot be fetched. Only H.264 MP4 is
 * accepted: Óia's native AVPlayer paths cannot play a WebM recording.
 */
export async function recordVideoElement(
  doc: Document,
  blobUrl: string,
  environment: VideoElementRecorderEnvironment = {},
): Promise<Response> {
  const video = findVideoElement(doc, blobUrl);
  if (!video) throw new Error("The temporary video element is no longer on this page.");
  if (!Number.isFinite(video.duration) || video.duration <= 0) {
    throw new Error("The temporary video's duration could not be determined.");
  }

  const capture = video.captureStream ?? video.mozCaptureStream;
  if (!capture) {
    throw new Error("This browser cannot copy the temporary video from this page.");
  }

  const Recorder = environment.MediaRecorder ?? globalThis.MediaRecorder;
  if (!Recorder) {
    throw new Error("This browser cannot record the temporary video.");
  }

  const scheduleTimeout = environment.setTimeout ?? setTimeout;
  const cancelTimeout = environment.clearTimeout ?? clearTimeout;
  const snapshot = snapshotVideo(video);
  const signal = environment.signal;
  signal?.throwIfAborted();
  let stream: MediaStream | undefined;
  let recorder: MediaRecorder;
  let actualMimeType: string;

  try {
    video.loop = true;
    video.playbackRate = 1;
    if (snapshot.paused) video.muted = true;

    // A paused MediaSource element can expose a capture track that never
    // produces frames. Start the exact page element before asking the browser
    // for its stream, then wait for any initially-muted video track.
    await waitForVideoTransfer(
      video.play(),
      "The temporary video did not start playing.",
      VIDEO_TRANSFER_TIMEOUT_MS,
      signal,
    );
    signal?.throwIfAborted();
    stream = capture.call(video);
    const videoTracks = stream.getVideoTracks();
    if (videoTracks.length === 0) {
      throw new Error("The temporary video did not expose a recordable video track.");
    }
    await waitForRecordableTrack(videoTracks[0], scheduleTimeout, cancelTimeout, signal);

    const requestedMimeType = selectMp4RecorderMimeType(
      Recorder,
      stream.getAudioTracks().length > 0,
    );
    if (!requestedMimeType) {
      throw new Error("This browser cannot create a Óia-compatible MP4 video.");
    }

    recorder = new Recorder(stream, { mimeType: requestedMimeType });
    actualMimeType = recorder.mimeType || requestedMimeType;
    if (!isH264Mp4(actualMimeType, requestedMimeType)) {
      throw new Error("The browser selected a video format Óia cannot play.");
    }
  } catch (error) {
    stream?.getTracks().forEach((track) => track.stop());
    restoreVideo(video, snapshot);
    throw error;
  }

  const body = await createRecorderStream(
    video,
    stream,
    recorder,
    scheduleTimeout,
    cancelTimeout,
    snapshot,
    signal,
  );
  return new Response(body, {
    status: 200,
    headers: { "Content-Type": actualMimeType },
  });
}

export function findVideoElement(doc: Document, blobUrl: string): CapturableVideo | undefined {
  return Array.from(doc.querySelectorAll("video")).find((candidate) => {
    const video = candidate as CapturableVideo;
    return (
      video.currentSrc === blobUrl || video.src === blobUrl || video.getAttribute("src") === blobUrl
    );
  }) as CapturableVideo | undefined;
}

export function selectMp4RecorderMimeType(
  Recorder: Pick<MediaRecorderConstructor, "isTypeSupported">,
  hasAudio: boolean,
): string | undefined {
  const videoCodecs = ["avc1.42E01F", "avc1.42E01E", "avc1"];
  const candidates = hasAudio
    ? videoCodecs.flatMap((codec) => [
        `video/mp4;codecs="${codec},mp4a.40.2"`,
        `video/mp4;codecs=${codec},mp4a.40.2`,
      ])
    : videoCodecs.flatMap((codec) => [`video/mp4;codecs="${codec}"`, `video/mp4;codecs=${codec}`]);
  return candidates.find((candidate) => Recorder.isTypeSupported(candidate));
}

async function createRecorderStream(
  video: HTMLVideoElement,
  mediaStream: MediaStream,
  recorder: MediaRecorder,
  scheduleTimeout: typeof setTimeout,
  cancelTimeout: typeof clearTimeout,
  snapshot: VideoSnapshot,
  signal?: AbortSignal,
): Promise<ReadableStream<Uint8Array>> {
  const duration = video.duration;
  const blobs: Blob[] = [];
  let waiting:
    | {
        resolve: (blob: Blob | undefined) => void;
      }
    | undefined;
  let blobReader: ReadableStreamDefaultReader<Uint8Array> | undefined;
  let elapsedMediaTime = 0;
  let lastMediaTime = video.currentTime;
  let stopped = false;
  let cancelled = false;
  let completionRequested = false;
  let restored = false;
  let pausedForBackpressure = false;
  let terminalError: Error | undefined;

  const restore = (): void => {
    if (restored) return;
    restored = true;
    video.removeEventListener("timeupdate", handleProgress);
    video.removeEventListener("error", handleVideoError);
    mediaStream.getTracks().forEach((track) => {
      track.removeEventListener("ended", handleTrackEnded);
      track.stop();
    });
    cancelTimeout(wallTimer);
    signal?.removeEventListener("abort", handleAbort);

    restoreVideo(video, snapshot);
  };

  const finishQueue = (error?: Error): void => {
    terminalError ??= error;
    stopped = true;
    waiting?.resolve(undefined);
    waiting = undefined;
  };

  const stopRecorder = (): void => {
    if (stopped || recorder.state === "inactive") return;
    completionRequested = true;
    stopped = true;
    recorder.stop();
  };

  const fail = (error: Error): void => {
    terminalError ??= error;
    cancelled = true;
    if (recorder.state !== "inactive") {
      try {
        recorder.stop();
      } catch {
        // The recorder may have transitioned to inactive between the state
        // check and stop call.
      }
    }
    finishQueue(error);
    restore();
  };

  function handleProgress(): void {
    if (stopped || video.paused) return;
    const current = video.currentTime;
    if (!Number.isFinite(current)) return;
    const delta =
      current >= lastMediaTime ? current - lastMediaTime : duration - lastMediaTime + current;
    if (delta >= 0 && delta <= duration) elapsedMediaTime += delta;
    lastMediaTime = current;
    const tolerance = Math.min(0.05, duration / 100);
    if (elapsedMediaTime >= duration - tolerance) stopRecorder();
  }

  function handleVideoError(): void {
    fail(new Error("The page stopped playing the temporary video."));
  }

  function handleTrackEnded(): void {
    if (!stopped) fail(new Error("The temporary video stream ended before it was copied."));
  }

  function handleAbort(): void {
    fail(
      signal?.reason instanceof Error
        ? signal.reason
        : new Error("The temporary video copy was cancelled."),
    );
  }

  recorder.addEventListener("dataavailable", (event) => {
    const blob = (event as BlobEvent).data;
    if (cancelled || !blob?.size) return;
    if (!stopped) {
      try {
        if (recorder.state === "recording") recorder.pause();
      } catch {
        // A simultaneous stop wins over the backpressure pause.
      }
      video.pause();
      pausedForBackpressure = true;
    }
    if (waiting) {
      waiting.resolve(blob);
      waiting = undefined;
    } else if (blobs.length === 0) {
      blobs.push(blob);
    } else {
      fail(new Error("The browser produced video data faster than it could be saved."));
    }
  });
  recorder.addEventListener("stop", () => {
    finishQueue(
      !completionRequested && !cancelled && !terminalError
        ? new Error("The temporary video stopped before a complete copy was recorded.")
        : undefined,
    );
    restore();
  });
  recorder.addEventListener("error", (event) => {
    const recorderError = (event as Event & { error?: DOMException }).error;
    fail(recorderError ?? new Error("The browser could not record the temporary video."));
  });
  video.addEventListener("timeupdate", handleProgress);
  video.addEventListener("error", handleVideoError);
  mediaStream.getTracks().forEach((track) => track.addEventListener("ended", handleTrackEnded));

  const wallTime = Math.min(
    MAX_RECORDING_WALL_TIME_MS,
    Math.max(60_000, duration * 4_000 + 30_000),
  );
  const wallTimer = scheduleTimeout(() => {
    fail(new Error("The temporary video took too long to copy."));
  }, wallTime);
  signal?.addEventListener("abort", handleAbort, { once: true });

  const takeBlob = (): Promise<Blob | undefined> => {
    const next = blobs.shift();
    if (next) return Promise.resolve(next);
    if (stopped) return Promise.resolve(undefined);
    return new Promise((resolve) => {
      waiting = { resolve };
    });
  };

  const resumeCapture = async (): Promise<void> => {
    if (!pausedForBackpressure || stopped || terminalError) return;
    pausedForBackpressure = false;
    if (recorder.state === "paused") recorder.resume();
    try {
      await waitForVideoTransfer(
        video.play(),
        "The temporary video did not resume playing.",
        VIDEO_TRANSFER_TIMEOUT_MS,
        signal,
      );
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)));
      throw error;
    }
  };

  const body = new ReadableStream<Uint8Array>(
    {
      async pull(controller) {
        while (true) {
          if (blobReader) {
            const next = await blobReader.read();
            if (!next.done) {
              controller.enqueue(next.value);
              return;
            }
            blobReader.releaseLock();
            blobReader = undefined;
            await resumeCapture();
          }

          const blob = await takeBlob();
          if (blob) {
            blobReader = blob.stream().getReader();
            continue;
          }
          if (terminalError) controller.error(terminalError);
          else controller.close();
          return;
        }
      },
      async cancel() {
        cancelled = true;
        fail(new Error("The temporary video copy was cancelled."));
        if (blobReader) await cancelVideoReader(blobReader);
      },
    },
    { highWaterMark: 0 },
  );

  try {
    signal?.throwIfAborted();
    recorder.start(RECORDING_SLICE_MS);
  } catch (error) {
    fail(error instanceof Error ? error : new Error(String(error)));
    throw error;
  }

  return body;
}

function snapshotVideo(video: HTMLVideoElement): VideoSnapshot {
  return {
    loop: video.loop,
    muted: video.muted,
    paused: video.paused,
    currentTime: video.currentTime,
    playbackRate: video.playbackRate,
  };
}

function restoreVideo(video: HTMLVideoElement, snapshot: VideoSnapshot): void {
  video.pause();
  video.loop = snapshot.loop;
  video.muted = snapshot.muted;
  video.playbackRate = snapshot.playbackRate;
  try {
    video.currentTime = snapshot.currentTime;
  } catch {
    // A source removed during capture can no longer be seeked; the remaining
    // state is still restored and the import will already have failed.
  }
  if (!snapshot.paused) void video.play().catch(() => undefined);
}

function waitForRecordableTrack(
  track: MediaStreamTrack,
  scheduleTimeout: typeof setTimeout,
  cancelTimeout: typeof clearTimeout,
  signal?: AbortSignal,
): Promise<void> {
  if (!track.muted) return Promise.resolve();

  return new Promise((resolve, reject) => {
    let timer: ReturnType<typeof setTimeout> | undefined = undefined;
    const cleanup = (): void => {
      if (timer !== undefined) cancelTimeout(timer);
      track.removeEventListener("unmute", handleUnmute);
      signal?.removeEventListener("abort", handleAbort);
    };
    const handleUnmute = (): void => {
      cleanup();
      resolve();
    };
    const handleAbort = (): void => {
      cleanup();
      reject(signal?.reason ?? new Error("The temporary video copy was cancelled."));
    };
    track.addEventListener("unmute", handleUnmute);
    signal?.addEventListener("abort", handleAbort, { once: true });
    if (signal?.aborted) {
      handleAbort();
      return;
    }
    if (!track.muted) {
      handleUnmute();
      return;
    }
    timer = scheduleTimeout(() => {
      cleanup();
      reject(new Error("The temporary video did not start producing recordable frames."));
    }, VIDEO_TRACK_READY_TIMEOUT_MS);
  });
}

function isH264Mp4(actual: string, requested: string): boolean {
  const normalized = actual.toLowerCase();
  if (!normalized.startsWith("video/mp4")) return false;
  if (/\b(vp8|vp9|av01|opus|hvc1|hev1)\b/.test(normalized)) return false;
  return normalized.includes("avc1") || requested.toLowerCase().includes("avc1");
}
