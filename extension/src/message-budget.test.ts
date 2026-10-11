// SPDX-License-Identifier: MIT

import { describe, expect, it } from "vitest";

import {
  assertMessageBudget,
  fitMessageImages,
  MessageBudgetError,
  serializedMessageBytes,
} from "./message-budget.js";

const image = (url: string, base64 = "AQID") => ({
  url,
  content_type: "image/png",
  data_base64: base64,
});

describe("whole message budget", () => {
  it("counts UTF-8 bytes and JSON escapes rather than string characters", () => {
    const message = { markdown: 'Óia 😀\n"quoted"', images: [] };
    const bytes = Buffer.byteLength(JSON.stringify(message), "utf8");
    expect(serializedMessageBytes(message)).toBe(bytes);
    expect(bytes).toBeGreaterThan(JSON.stringify(message).length);
    expect(() => assertMessageBudget(message, bytes)).not.toThrow();
    expect(() => assertMessageBudget(message, bytes - 1)).toThrow(MessageBudgetError);
  });

  it("fits optional images exactly while preserving all Markdown and metadata", () => {
    const message = {
      markdown: "Complete article 😀",
      metadata: { title: 'A "quoted" title', product_html: "<p>All evidence</p>" },
      images: [image("first"), image("second"), image("third")],
    };
    const limit = serializedMessageBytes({ ...message, images: message.images.slice(0, 2) });
    const fitted = fitMessageImages(message, limit);
    expect(fitted.images).toEqual(message.images.slice(0, 2));
    expect(fitted.markdown).toBe(message.markdown);
    expect(fitted.metadata).toBe(message.metadata);
    expect(fitted).not.toBe(message);
    expect(message.images).toHaveLength(3);
    expect(serializedMessageBytes(fitted)).toBe(limit);
  });

  it("can skip one large optional image and retain a later smaller candidate", () => {
    const message = { markdown: "Body", images: [image("large", "A".repeat(100)), image("small")] };
    const limit = serializedMessageBytes({ ...message, images: [message.images[1]] });
    expect(fitMessageImages(message, limit).images).toEqual([message.images[1]]);
  });

  it("explicitly fails when complete text or evidence cannot fit even without images", () => {
    const message = {
      markdown: "Full content",
      page_content: { text_characters: 12 },
      images: [image("preview")],
    };
    const baseline = serializedMessageBytes({ ...message, images: [] });
    expect(() => fitMessageImages(message, baseline - 1)).toThrow(MessageBudgetError);
    expect(fitMessageImages(message, baseline).images).toEqual([]);
  });
});
