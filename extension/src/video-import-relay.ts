// SPDX-License-Identifier: MIT

import {
  PROTOCOL_VERSION,
  type VideoImportAbortRequest,
  type VideoImportRequest,
  type VideoImportResponse,
} from "./protocol.js";
import {
  VIDEO_PREPARATION_TIMEOUT_MS,
  VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS,
  VIDEO_TRANSFER_FINISH_TIMEOUT_MS,
  VIDEO_TRANSFER_TIMEOUT_MS,
} from "./video-transfer.js";
import type { VideoImportPreparationMessage } from "./video-import.js";

interface ListenerEvent<T extends (...args: never[]) => void> {
  addListener(listener: T): void;
  removeListener(listener: T): void;
}

/** Runtime/native Port surface used by the service-worker relay. */
export interface RelayPort {
  postMessage(message: unknown): void;
  disconnect(): void;
  onMessage: ListenerEvent<(message: unknown) => void>;
  onDisconnect: ListenerEvent<() => void>;
}

/**
 * Relay one tab-owned video upload over one persistent native connection.
 * Messages are serialized: even a misbehaving tab cannot put the next chunk on
 * the native port until the preceding request has received its acknowledgement.
 */
export function relayVideoImportPort(
  tabPort: RelayPort,
  connectNative: () => RelayPort,
  nativeDisconnectMessage: () => string | undefined = () => undefined,
): void {
  let nativePort: RelayPort;
  try {
    nativePort = connectNative();
  } catch (error) {
    postError(tabPort, error);
    tabPort.disconnect();
    return;
  }

  const queued: VideoImportRequest[] = [];
  let pending: VideoImportRequest | undefined;
  let uploadId: string | undefined;
  let tabDisconnected = false;
  let uploadTerminal = false;
  let abortQueued = false;
  let closed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const preparationDeadline = Date.now() + VIDEO_PREPARATION_TIMEOUT_MS;

  const close = (nativeDisconnected = false): void => {
    if (closed) return;
    closed = true;
    clearTimeout(timer);
    queued.length = 0;
    pending = undefined;
    tabPort.onMessage.removeListener(handleTabMessage);
    tabPort.onDisconnect.removeListener(handleTabDisconnect);
    nativePort.onMessage.removeListener(handleNativeMessage);
    nativePort.onDisconnect.removeListener(handleNativeDisconnect);
    // Native EOF removes unfinished staging even when an abort ACK is stuck.
    if (!nativeDisconnected) nativePort.disconnect();
    if (!tabDisconnected) {
      tabDisconnected = true;
      tabPort.disconnect();
    }
  };

  const arm = (timeoutMs = VIDEO_TRANSFER_TIMEOUT_MS): void => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (tabDisconnected) {
        close();
        return;
      }
      postError(tabPort, new Error("The video import stopped making progress."), "video_timeout");
      tabPort.disconnect();
      handleTabDisconnect();
    }, timeoutMs);
  };

  const flush = (): void => {
    if (closed || pending) return;

    if (tabDisconnected && uploadId && !uploadTerminal && !abortQueued) {
      const abort: VideoImportAbortRequest = {
        protocol_version: PROTOCOL_VERSION,
        action: "video_import_abort",
        upload_id: uploadId,
      };
      queued.length = 0;
      queued.push(abort);
      abortQueued = true;
    }

    const next = queued.shift();
    if (!next) {
      if (tabDisconnected) close();
      else arm();
      return;
    }

    pending = next;
    arm(
      tabDisconnected
        ? VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS
        : next.action === "video_import_finish"
          ? VIDEO_TRANSFER_FINISH_TIMEOUT_MS
          : VIDEO_TRANSFER_TIMEOUT_MS,
    );
    try {
      nativePort.postMessage(next);
    } catch (error) {
      pending = undefined;
      if (!tabDisconnected) postError(tabPort, error);
      close();
    }
  };

  const handleTabMessage = (value: unknown): void => {
    if (closed || tabDisconnected) return;
    if (isPreparationMessage(value)) {
      // Preparing can outlast a normal read, but it is bounded and never masks
      // an ACK deadline once any native upload request has been sent.
      if (!uploadId && !pending) {
        arm(Math.max(0, Math.min(VIDEO_TRANSFER_TIMEOUT_MS, preparationDeadline - Date.now())));
      }
      return;
    }
    if (!isVideoImportRequest(value)) {
      postError(tabPort, new Error("The tab sent an invalid video import message."));
      return;
    }
    if (value.action === "video_import_begin") uploadId = value.upload_id;
    queued.push(value);
    flush();
  };

  const handleNativeMessage = (value: unknown): void => {
    if (closed) return;
    const completed = pending;
    pending = undefined;
    if (completed?.action === "video_import_finish" || completed?.action === "video_import_abort") {
      uploadTerminal = true;
    }
    if (!tabDisconnected) {
      try {
        tabPort.postMessage(value as VideoImportResponse);
      } catch {
        handleTabDisconnect();
      }
    }
    flush();
  };

  const handleTabDisconnect = (): void => {
    if (closed) return;
    tabDisconnected = true;
    queued.length = 0;
    if (pending) {
      // Give an in-flight ACK a short chance to arrive so abort can be sent,
      // then force EOF instead of retaining a native port indefinitely.
      arm(VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS);
    }
    flush();
  };

  const handleNativeDisconnect = (): void => {
    if (closed) return;
    pending = undefined;
    queued.length = 0;
    if (!tabDisconnected) {
      postError(
        tabPort,
        new Error(nativeDisconnectMessage() ?? "The Óia app closed the video import connection."),
      );
    }
    close(true);
  };

  tabPort.onMessage.addListener(handleTabMessage);
  tabPort.onDisconnect.addListener(handleTabDisconnect);
  nativePort.onMessage.addListener(handleNativeMessage);
  nativePort.onDisconnect.addListener(handleNativeDisconnect);
  arm();
}

function isVideoImportRequest(value: unknown): value is VideoImportRequest {
  if (!value || typeof value !== "object") return false;
  const message = value as Partial<VideoImportRequest>;
  return (
    message.protocol_version === PROTOCOL_VERSION &&
    typeof message.upload_id === "string" &&
    (message.action === "video_import_begin" ||
      message.action === "video_import_chunk" ||
      message.action === "video_import_finish" ||
      message.action === "video_import_abort")
  );
}

function isPreparationMessage(value: unknown): value is VideoImportPreparationMessage {
  if (!value || typeof value !== "object") return false;
  const message = value as Partial<VideoImportPreparationMessage>;
  return (
    message.protocol_version === PROTOCOL_VERSION &&
    message.action === "video_import_prepare" &&
    typeof message.upload_id === "string"
  );
}

function postError(port: RelayPort, error: unknown, code = "native_connection"): void {
  try {
    port.postMessage({
      protocol_version: PROTOCOL_VERSION,
      ok: false,
      error: code,
      message: error instanceof Error ? error.message : String(error),
    } satisfies VideoImportResponse);
  } catch {
    // The tab may already be gone; there is nowhere left to surface the error.
  }
}
