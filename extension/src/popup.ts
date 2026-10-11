// SPDX-License-Identifier: MIT

import { isToolbarSaveKind, toolbarSaveMessage, type ToolbarSaveKind } from "./toolbar.js";
import { isWebPage } from "./capture-context.js";

export function initializePopup(): void {
  const buttons = Array.from(
    document.querySelectorAll<HTMLButtonElement>("button[data-save-kind]"),
  );
  const errorMessage = document.getElementById("save-error");

  buttons[0]?.focus();

  for (const button of buttons) {
    button.addEventListener("click", async () => {
      const kind = button.dataset.saveKind;
      if (!isToolbarSaveKind(kind)) return;

      setDisabled(buttons, true);
      if (errorMessage) errorMessage.hidden = true;
      try {
        const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
        if (!tab?.id) throw new Error("The active tab could not be identified.");
        if (!isWebPage(tab.url)) {
          throw new Error(
            "This page isn't available to Óia. Open a regular web page and try again.",
          );
        }
        const response = (await chrome.runtime.sendMessage(
          toolbarSaveMessage(kind as ToolbarSaveKind, tab.id),
        )) as { accepted?: boolean; error?: string } | undefined;
        if (!response?.accepted) throw new Error(response?.error ?? "The page couldn't be saved.");
        window.close();
      } catch (error) {
        if (errorMessage) {
          errorMessage.textContent =
            error instanceof Error ? error.message : "The page couldn't be saved.";
          errorMessage.hidden = false;
        }
        setDisabled(buttons, false);
        button.focus();
      }
    });
  }
}

document.addEventListener("DOMContentLoaded", initializePopup);

function setDisabled(buttons: HTMLButtonElement[], disabled: boolean): void {
  for (const button of buttons) button.disabled = disabled;
}
