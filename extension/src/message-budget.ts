// SPDX-License-Identifier: MIT

import type { ImageData } from "./protocol.js";

/** Chrome's browser → native-host limit applies to the complete UTF-8 JSON message. */
export const MAX_MESSAGE_BYTES = 64 * 1024 * 1024;

export class MessageBudgetError extends Error {
  readonly code = "capture_too_large";

  constructor() {
    super("This page is too large to save completely.");
    this.name = "MessageBudgetError";
  }
}

export function serializedMessageBytes(value: unknown): number {
  const json = JSON.stringify(value);
  if (json === undefined) throw new MessageBudgetError();
  return new TextEncoder().encode(json).byteLength;
}

export function assertMessageBudget(value: unknown, maxBytes = MAX_MESSAGE_BYTES): void {
  if (serializedMessageBytes(value) > maxBytes) throw new MessageBudgetError();
}

/**
 * Preserve all text and evidence, fitting optional images in their existing priority
 * order. Baseline JSON and each image are measured once, including UTF-8 and escapes.
 */
export function fitMessageImages<T extends { images: ImageData[] }>(
  message: T,
  maxBytes = MAX_MESSAGE_BYTES,
): T {
  const baseline = { ...message, images: [] as ImageData[] };
  let bytes = serializedMessageBytes(baseline);
  if (bytes > maxBytes) throw new MessageBudgetError();
  const images: ImageData[] = [];
  for (const image of message.images) {
    const imageBytes = serializedMessageBytes(image) + (images.length ? 1 : 0);
    if (bytes + imageBytes > maxBytes) continue;
    bytes += imageBytes;
    images.push(image);
  }
  return { ...message, images };
}
