// SPDX-License-Identifier: MIT

import {
  PAGE_VIDEO_READY_TIMEOUT_MS,
  VIDEO_TRANSFER_TIMEOUT_MS,
  waitForVideoTransfer,
} from "./video-transfer.js";

export const PREPARE_PAGE_VIDEO_BRIDGE = "prepare-page-video-bridge";
export const OPEN_PAGE_VIDEO_SOURCE = "oia:open-page-video-source:v1";

interface PageVideoReadyMessage {
  type: "ready";
  ok: true;
  contentType: string;
  expectedBytes?: number;
}

interface PageVideoErrorMessage {
  type: "error";
  ok: false;
  message: string;
}

interface PageVideoChunkMessage {
  type: "chunk";
  data: ArrayBuffer;
}

interface PageVideoDoneMessage {
  type: "done";
}

type PageVideoMessage =
  | PageVideoReadyMessage
  | PageVideoErrorMessage
  | PageVideoChunkMessage
  | PageVideoDoneMessage;

interface BridgePreparationResponse {
  ok: boolean;
  message?: string;
}

type PrepareBridge = () => Promise<void>;

/**
 * Read or record a selected video from the page's MAIN execution world. Some
 * WebExtension hosts keep object-URL lookup or element capture isolated from
 * content scripts even though both worlds can see the same `<video>` element.
 *
 * The MAIN-world bridge still streams on demand: one `next` request produces
 * at most one source chunk, so this adapter never materializes the whole video.
 */
export async function fetchPageVideoSource(
  url: string,
  prepareBridge: PrepareBridge = preparePageVideoBridge,
  targetWindow: Window = window,
  signal?: AbortSignal,
): Promise<Response> {
  await waitForVideoTransfer(
    prepareBridge(),
    "The page video bridge did not respond.",
    VIDEO_TRANSFER_TIMEOUT_MS,
    signal,
  );
  signal?.throwIfAborted();

  const channel = new MessageChannel();
  const cancellation = new AbortController();
  let closed = false;
  let bodyController: ReadableStreamDefaultController<Uint8Array> | undefined;
  const close = (reason?: unknown, notifyPage = true): void => {
    if (closed) return;
    closed = true;
    if (notifyPage) {
      try {
        channel.port1.postMessage({ type: "cancel" });
      } catch {
        // The MAIN endpoint may already have closed or never have opened.
      }
    }
    cancellation.abort(reason);
    signal?.removeEventListener("abort", aborted);
    channel.port1.close();
    channel.port2.close();
  };
  const aborted = (): void => {
    const reason = signal?.reason ?? new Error("The page video capture was cancelled.");
    close(reason);
    bodyController?.error(reason);
  };
  signal?.addEventListener("abort", aborted, { once: true });
  if (signal?.aborted) aborted();
  channel.port1.start();
  const readyPromise = receivePageVideoMessage(
    channel.port1,
    PAGE_VIDEO_READY_TIMEOUT_MS,
    cancellation.signal,
  );
  let ready: PageVideoReadyMessage;
  try {
    targetWindow.postMessage({ type: OPEN_PAGE_VIDEO_SOURCE, url }, "*", [channel.port2]);
    const message = await readyPromise;
    cancellation.signal.throwIfAborted();
    if (message.type === "error") throw new TypeError(message.message);
    if (message.type !== "ready" || !message.ok) {
      throw new TypeError("The page did not expose the temporary video source.");
    }
    ready = message;
  } catch (error) {
    close(error);
    // If postMessage itself threw, still observe the aborted ready listener.
    void readyPromise.catch(() => undefined);
    throw error;
  }

  const body = new ReadableStream<Uint8Array>(
    {
      start(controller) {
        bodyController = controller;
      },
      async pull(controller) {
        try {
          const pending = receivePageVideoMessage(
            channel.port1,
            VIDEO_TRANSFER_TIMEOUT_MS,
            cancellation.signal,
          );
          void pending.catch(() => undefined);
          channel.port1.postMessage({ type: "next" });
          const message = await pending;
          if (closed) return;
          if (message.type === "chunk") {
            controller.enqueue(new Uint8Array(message.data));
            return;
          }
          if (message.type === "done") {
            close(undefined, false);
            controller.close();
            return;
          }
          throw new Error(
            message.type === "error"
              ? message.message
              : "The page returned an invalid video stream response.",
          );
        } catch (error) {
          if (closed) return;
          close(error);
          controller.error(error);
        }
      },
      cancel(reason) {
        close(reason);
      },
    },
    { highWaterMark: 0 },
  );

  const headers = new Headers({ "Content-Type": ready.contentType });
  if (ready.expectedBytes !== undefined) {
    headers.set("Content-Length", String(ready.expectedBytes));
  }
  return new Response(body, { status: 200, headers });
}

export function isPreparePageVideoBridgeMessage(
  value: unknown,
): value is { action: typeof PREPARE_PAGE_VIDEO_BRIDGE } {
  return (
    Boolean(value) &&
    typeof value === "object" &&
    (value as { action?: unknown }).action === PREPARE_PAGE_VIDEO_BRIDGE
  );
}

function preparePageVideoBridge(): Promise<void> {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendMessage({ action: PREPARE_PAGE_VIDEO_BRIDGE }, (response) => {
      if (chrome.runtime.lastError) {
        reject(new Error(chrome.runtime.lastError.message));
        return;
      }
      const prepared = response as BridgePreparationResponse | undefined;
      if (!prepared?.ok) {
        reject(new Error(prepared?.message || "The page video bridge could not be started."));
        return;
      }
      resolve();
    });
  });
}

function receivePageVideoMessage(
  port: MessagePort,
  timeoutMs: number,
  signal: AbortSignal,
): Promise<PageVideoMessage> {
  let cleanup = (): void => undefined;
  const pending = new Promise<PageVideoMessage>((resolve, reject) => {
    const handleMessage = (event: MessageEvent<unknown>): void => {
      cleanup();
      resolve(event.data as PageVideoMessage);
    };
    const handleMessageError = (): void => {
      cleanup();
      reject(new Error("The page video stream could not be decoded."));
    };
    cleanup = (): void => {
      port.removeEventListener("message", handleMessage);
      port.removeEventListener("messageerror", handleMessageError);
    };
    port.addEventListener("message", handleMessage);
    port.addEventListener("messageerror", handleMessageError);
  });
  return waitForVideoTransfer(
    pending,
    "The page stopped responding to the video capture.",
    timeoutMs,
    signal,
  ).finally(cleanup);
}
