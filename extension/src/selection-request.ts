// SPDX-License-Identifier: MIT

import type { SelectionCapture } from "./selection-capture.js";
import type { SaveSelectionRequest } from "./protocol.js";
import { PROTOCOL_VERSION } from "./protocol.js";

/** Keep selection saves distinct from ordinary article and lightweight-link saves. */
export function buildSaveSelectionRequest(capture: SelectionCapture): SaveSelectionRequest {
  return {
    protocol_version: PROTOCOL_VERSION,
    action: "save_selection",
    metadata: capture.metadata,
    markdown: capture.markdown,
    images: capture.images,
    preview_url: capture.preview_url,
    favicon_url: capture.favicon_url,
    selected_text: capture.selected_text,
    selection_context: capture.selection_context,
  };
}
