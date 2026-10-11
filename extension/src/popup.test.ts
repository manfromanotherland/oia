// SPDX-License-Identifier: MIT

import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function clickSave(url?: string, response: object = { accepted: true }) {
  vi.resetModules();
  document.body.innerHTML =
    '<button data-save-kind="page">Save</button><button data-save-kind="screenshot">Save as screenshot</button><p id="save-error" role="alert" hidden></p>';
  const sendMessage = vi.fn(async () => response);
  vi.stubGlobal("chrome", {
    tabs: { query: async () => [{ id: 42, url }] },
    runtime: { sendMessage },
  });
  const close = vi.spyOn(window, "close").mockImplementation(() => undefined);
  const { initializePopup } = await import("./popup.js");
  initializePopup();
  const button = document.querySelector("button")!;
  button.click();
  await vi.waitFor(() => expect(button.disabled).toBe(false));
  return { sendMessage, close, error: document.getElementById("save-error")! };
}

describe("popup failures", () => {
  it.each([undefined, "chrome://extensions", "about:blank"])(
    "explains unsupported page %s without silently closing",
    async (url) => {
      const result = await clickSave(url);
      expect(result.sendMessage).not.toHaveBeenCalled();
      expect(result.close).not.toHaveBeenCalled();
      expect(result.error.hidden).toBe(false);
      expect(result.error.textContent).toContain("regular web page");
    },
  );

  it("shows the background's rejection reason and keeps the menu usable", async () => {
    const result = await clickSave("https://example.com", {
      accepted: false,
      error: "The source tab is no longer available.",
    });
    expect(result.close).not.toHaveBeenCalled();
    expect(result.error.textContent).toBe("The source tab is no longer available.");
  });
});
