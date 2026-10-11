// SPDX-License-Identifier: MIT

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PROTOCOL_VERSION, type VideoImportRequest, type VideoImportResponse } from "./protocol.js";
import { relayVideoImportPort, type RelayPort } from "./video-import-relay.js";
import {
  VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS,
  VIDEO_TRANSFER_FINISH_TIMEOUT_MS,
  VIDEO_TRANSFER_TIMEOUT_MS,
  VIDEO_PREPARATION_HEARTBEAT_MS,
  VIDEO_PREPARATION_TIMEOUT_MS,
} from "./video-transfer.js";

beforeEach(() => vi.useFakeTimers());
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

  get size(): number {
    return this.listeners.size;
  }
}

class FakeRelayPort implements RelayPort {
  readonly onMessage = new ListenerEvent<(message: unknown) => void>();
  readonly onDisconnect = new ListenerEvent<() => void>();
  readonly sent: unknown[] = [];
  disconnected = false;

  postMessage(message: unknown): void {
    this.sent.push(message);
  }

  disconnect(): void {
    this.disconnected = true;
  }

  receive(message: unknown): void {
    this.onMessage.emit(message);
  }

  drop(): void {
    this.onDisconnect.emit();
  }
}

const ack: VideoImportResponse = { protocol_version: PROTOCOL_VERSION, ok: true };

function begin(): VideoImportRequest {
  return {
    protocol_version: PROTOCOL_VERSION,
    action: "video_import_begin",
    upload_id: "one-upload",
    metadata: {
      kind: "video",
      url: "https://www.cosmos.so/e/2035271300",
      canonical_url: "https://www.cosmos.so/e/2035271300",
      title: "Cosmos",
      saved_at: "2026-08-25T15:00:00.000Z",
    },
    content_type: "video/mp4",
  };
}

function chunk(sequence: number): VideoImportRequest {
  return {
    protocol_version: PROTOCOL_VERSION,
    action: "video_import_chunk",
    upload_id: "one-upload",
    sequence,
    data_base64: "AA==",
  };
}

describe("video import worker relay", () => {
  it("opens one native port and does not forward another chunk before its acknowledgement", () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    const connectNative = vi.fn(() => native);
    relayVideoImportPort(tab, connectNative);

    tab.receive(begin());
    tab.receive(chunk(0));
    tab.receive(chunk(1));
    expect(connectNative).toHaveBeenCalledTimes(1);
    expect(native.sent).toEqual([begin()]);

    native.receive(ack);
    expect(tab.sent).toEqual([ack]);
    expect(native.sent).toEqual([begin(), chunk(0)]);

    native.receive(ack);
    expect(tab.sent).toEqual([ack, ack]);
    expect(native.sent).toEqual([begin(), chunk(0), chunk(1)]);
  });

  it("waits for an in-flight acknowledgement, then aborts the same upload when the tab port dies", () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    relayVideoImportPort(tab, () => native);

    tab.receive(begin());
    tab.drop();
    expect(native.sent).toEqual([begin()]);

    native.receive(ack);
    expect(native.sent).toEqual([
      begin(),
      {
        protocol_version: PROTOCOL_VERSION,
        action: "video_import_abort",
        upload_id: "one-upload",
      },
    ]);

    native.receive(ack);
    expect(native.disconnected).toBe(true);
  });

  it("forwards the browser's native disconnect reason to the tab", () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    relayVideoImportPort(
      tab,
      () => native,
      () => "Specified native messaging host not found.",
    );

    native.drop();

    expect(tab.sent).toEqual([
      {
        protocol_version: PROTOCOL_VERSION,
        ok: false,
        error: "native_connection",
        message: "Specified native messaging host not found.",
      },
    ]);
    expect(tab.disconnected).toBe(true);
  });

  it("closes a native connection if the tab never begins its upload", async () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    relayVideoImportPort(tab, () => native);

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    expect(tab.sent.at(-1)).toMatchObject({ ok: false, error: "video_timeout" });
    expect(tab.disconnected).toBe(true);
    expect(native.disconnected).toBe(true);
    expect(native.onMessage.size).toBe(0);
    expect(tab.onDisconnect.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps preparation alive through local heartbeats without forwarding them to native", async () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    relayVideoImportPort(tab, () => native);
    const preparing = {
      protocol_version: PROTOCOL_VERSION,
      action: "video_import_prepare",
      upload_id: "one-upload",
    };
    for (let tick = 0; tick < 6; tick += 1) {
      await vi.advanceTimersByTimeAsync(VIDEO_PREPARATION_HEARTBEAT_MS);
      tab.receive(preparing);
    }
    expect(native.sent).toEqual([]);
    expect(native.disconnected).toBe(false);

    tab.receive(begin());
    await vi.advanceTimersByTimeAsync(40_000);
    tab.receive(preparing);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(tab.sent.at(-1)).toMatchObject({ error: "video_timeout" });
    expect(tab.disconnected).toBe(true);
    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS);
    expect(native.disconnected).toBe(true);
    expect(native.sent).toEqual([begin()]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("bounds preparation even if a broken tab keeps sending heartbeats forever", async () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    relayVideoImportPort(tab, () => native);
    for (
      let elapsed = 0;
      elapsed <= VIDEO_PREPARATION_TIMEOUT_MS;
      elapsed += VIDEO_PREPARATION_HEARTBEAT_MS
    ) {
      await vi.advanceTimersByTimeAsync(VIDEO_PREPARATION_HEARTBEAT_MS);
      tab.receive({
        protocol_version: PROTOCOL_VERSION,
        action: "video_import_prepare",
        upload_id: "one-upload",
      });
    }
    expect(tab.disconnected).toBe(true);
    expect(native.disconnected).toBe(true);
    expect(native.sent).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("forces native EOF if the tab disconnects during an ACK that never arrives", async () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    relayVideoImportPort(tab, () => native);
    tab.receive(begin());
    tab.drop();

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS);
    expect(native.disconnected).toBe(true);
    expect(native.sent).toEqual([begin()]);
    expect(native.onMessage.size).toBe(0);
    expect(tab.onMessage.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("times out a missing native ACK and bounds the following disconnect cleanup", async () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    relayVideoImportPort(tab, () => native);
    tab.receive(begin());

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    expect(tab.disconnected).toBe(true);
    expect(tab.sent.at(-1)).toMatchObject({ error: "video_timeout" });
    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS);
    expect(native.disconnected).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("aborts an idle started upload and forces EOF if abort is not acknowledged", async () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    relayVideoImportPort(tab, () => native);
    tab.receive(begin());
    native.receive(ack);

    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_TIMEOUT_MS);
    expect(native.sent.at(-1)).toMatchObject({
      action: "video_import_abort",
      upload_id: "one-upload",
    });
    expect(tab.disconnected).toBe(true);
    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_CLEANUP_TIMEOUT_MS);
    expect(native.disconnected).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("renews inactivity after each acknowledged chunk and gives finalize more time", async () => {
    const tab = new FakeRelayPort();
    const native = new FakeRelayPort();
    relayVideoImportPort(tab, () => native);
    tab.receive(begin());
    native.receive(ack);
    for (let sequence = 0; sequence < 3; sequence += 1) {
      await vi.advanceTimersByTimeAsync(40_000);
      tab.receive(chunk(sequence));
      await vi.advanceTimersByTimeAsync(40_000);
      native.receive(ack);
    }
    expect(native.disconnected).toBe(false);
    expect(tab.disconnected).toBe(false);

    tab.receive({
      protocol_version: PROTOCOL_VERSION,
      action: "video_import_finish",
      upload_id: "one-upload",
    });
    await vi.advanceTimersByTimeAsync(VIDEO_TRANSFER_FINISH_TIMEOUT_MS - 1);
    expect(native.disconnected).toBe(false);
    native.receive({ ...ack, id: "saved-video", path: "article.md" });
    tab.drop();
    expect(native.disconnected).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });
});
