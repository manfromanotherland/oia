// SPDX-License-Identifier: MIT

import type { PageCapture } from "./content.js";
import type { InspectPageMetadataRequest, InspectPageMetadataResponse } from "./protocol.js";
import { PROTOCOL_VERSION } from "./protocol.js";
import { MAX_PRODUCT_HTML_BYTES } from "./product-html.js";

const INSPECTION_TIMEOUT_MS = 2_000;
const MAX_PREVIEW_CANDIDATES = 4;

/** Ask Rust for the main entity's artwork, then capture it with ordinary images. */
export async function inspectCaptureArtwork(
  capture: PageCapture,
  inspect: (request: InspectPageMetadataRequest) => Promise<InspectPageMetadataResponse>,
): Promise<void> {
  const evidence = capture.metadata.product_html;
  if (
    capture.metadata.kind !== "article" ||
    !evidence ||
    new TextEncoder().encode(evidence).byteLength > MAX_PRODUCT_HTML_BYTES
  ) {
    return;
  }

  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const response = await Promise.race([
      inspect({
        protocol_version: PROTOCOL_VERSION,
        action: "inspect_page_metadata",
        url: capture.metadata.url,
        product_html: evidence,
      }),
      new Promise<undefined>((resolve) => {
        timer = setTimeout(() => resolve(undefined), INSPECTION_TIMEOUT_MS);
      }),
    ]);
    if (!response?.ok || response.protocol_version !== PROTOCOL_VERSION) return;
    if (!Array.isArray(response.preview_candidates)) return;
    const candidates = [
      ...new Set(
        response.preview_candidates.slice(0, MAX_PREVIEW_CANDIDATES).flatMap((value) => {
          if (typeof value !== "string") return [];
          try {
            const url = new URL(value);
            return (url.protocol === "https:" || url.protocol === "http:") &&
              !url.username &&
              !url.password
              ? [url.href]
              : [];
          } catch {
            return [];
          }
        }),
      ),
    ];
    capture.preview_candidates = [
      ...new Set([...candidates, ...(capture.preview_candidates ?? [])]),
    ];
    capture.unresolved = [
      ...new Set([
        ...candidates.filter((url) => !capture.images.some((image) => image.url === url)),
        ...capture.unresolved,
      ]),
    ];
  } catch {
    // Inspection is additive: old hosts and unavailable hosts retain normal capture.
  } finally {
    clearTimeout(timer);
  }
}
