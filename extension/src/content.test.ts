// SPDX-License-Identifier: MIT

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { initializeContentScript, showToast } from "./content.js";

const HOST_ID = "oia-toast-host";

describe("showToast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-15T12:00:00Z"));
    document.body.innerHTML = "";
    Object.assign(chrome.runtime, {
      sendMessage: vi.fn().mockResolvedValue({ accepted: true }),
    });
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("renders the title and detail inside an isolated shadow root", () => {
    showToast({
      action: "toast",
      status: "ok",
      title: "Saved to Óia",
      detail: "My Article",
    });

    const host = document.getElementById(HOST_ID);
    expect(host).not.toBeNull();
    expect(host!.shadowRoot).not.toBeNull();
    expect(host!.shadowRoot!.textContent).toContain("Saved to Óia");
    expect(host!.shadowRoot!.textContent).toContain("My Article");
  });

  it("escapes page-derived text instead of injecting markup", () => {
    showToast({ action: "toast", status: "ok", title: "<img src=x onerror=alert(1)>" });

    const root = document.getElementById(HOST_ID)!.shadowRoot!;
    expect(root.querySelector("img")).toBeNull();
    expect(root.querySelector(".title")!.textContent).toBe("<img src=x onerror=alert(1)>");
  });

  it("replaces an existing non-loading toast rather than stacking", () => {
    showToast({ action: "toast", status: "ok", title: "First" });
    showToast({ action: "toast", status: "error", title: "Second" });

    expect(document.querySelectorAll(`#${HOST_ID}`)).toHaveLength(1);
    expect(document.getElementById(HOST_ID)!.shadowRoot!.textContent).toContain("Second");
  });

  it("renders a spinner and no badge for loading status", () => {
    showToast({ action: "toast", status: "loading", title: "Saving…" });

    const root = document.getElementById(HOST_ID)!.shadowRoot!;
    expect(root.querySelector(".spinner")).not.toBeNull();
    expect(root.querySelector(".badge")).toBeNull();
    expect(root.querySelector(".title")!.textContent).toBe("Saving…");
  });

  it("includes a close button on every toast", () => {
    showToast({ action: "toast", status: "loading", title: "Saving…" });
    expect(document.getElementById(HOST_ID)!.shadowRoot!.querySelector(".close")).not.toBeNull();

    showToast({ action: "toast", status: "ok", title: "Saved" });
    expect(document.getElementById(HOST_ID)!.shadowRoot!.querySelector(".close")).not.toBeNull();
  });

  it("close button removes the host element", () => {
    showToast({ action: "toast", status: "ok", title: "Saved" });
    const host = document.getElementById(HOST_ID)!;
    host.shadowRoot!.querySelector<HTMLButtonElement>(".close")!.click();
    expect(document.getElementById(HOST_ID)).toBeNull();
  });

  it("transitions a loading toast to ok in place (same host element)", () => {
    showToast({ action: "toast", status: "loading", title: "Saving…" });
    const host = document.getElementById(HOST_ID)!;

    showToast({
      action: "toast",
      status: "ok",
      title: "Saved to Óia",
      detail: "My Article",
    });

    expect(document.getElementById(HOST_ID)).toBe(host);
    expect(document.querySelectorAll(`#${HOST_ID}`)).toHaveLength(1);
    const root = host.shadowRoot!;
    expect(root.querySelector(".badge")).not.toBeNull();
    expect(root.querySelector(".spinner")).toBeNull();
    expect(root.querySelector(".title")!.textContent).toBe("Saved to Óia");
    expect(root.querySelector(".detail")!.textContent).toBe("My Article");
    expect(root.querySelector(".toast")!.getAttribute("role")).toBe("status");
    expect(host.dataset.status).toBe("ok");
  });

  it("transitions a loading toast to error in place (same host element)", () => {
    showToast({ action: "toast", status: "loading", title: "Saving…" });
    const host = document.getElementById(HOST_ID)!;

    showToast({ action: "toast", status: "error", title: "Couldn't save page" });

    expect(document.getElementById(HOST_ID)).toBe(host);
    const root = host.shadowRoot!;
    expect(root.querySelector(".badge")).not.toBeNull();
    expect(root.querySelector(".spinner")).toBeNull();
    expect(root.querySelector(".title")!.textContent).toBe("Couldn't save page");
    expect(root.querySelector(".toast")!.getAttribute("role")).toBe("alert");
    expect(host.dataset.status).toBe("error");
  });

  it("shows a new result toast when the loading toast was dismissed before the result arrived", () => {
    showToast({ action: "toast", status: "loading", title: "Saving…" });
    document.getElementById(HOST_ID)!.remove();

    showToast({
      action: "toast",
      status: "ok",
      title: "Saved to Óia",
      detail: "My Article",
    });

    const host = document.getElementById(HOST_ID);
    expect(host).not.toBeNull();
    expect(host!.dataset.status).toBe("ok");
    expect(host!.shadowRoot!.querySelector(".badge")).not.toBeNull();
    expect(host!.shadowRoot!.querySelector(".spinner")).toBeNull();
    expect(host!.shadowRoot!.querySelector(".title")!.textContent).toBe("Saved to Óia");
  });

  function offerSaveChoice(expiresAt = Date.now() + 3000, onChoice = vi.fn()): HTMLElement {
    showToast(
      {
        action: "toast",
        status: "loading",
        title: "Saving page…",
        cta: {
          label: "Screenshot instead",
          command: "save-screenshot",
          expiresAt,
        },
      },
      onChoice,
    );
    return document.getElementById(HOST_ID)!;
  }

  it.each(["page", "screenshot"])(
    "keeps the save-choice message channel open until the %s choice",
    (kind) => {
      const addListener = vi.fn();
      Object.assign(chrome.runtime, { onMessage: { addListener } });
      initializeContentScript();
      const listener: Parameters<typeof chrome.runtime.onMessage.addListener>[0] =
        addListener.mock.calls[0][0];
      const sendResponse = vi.fn();
      expect(
        listener(
          {
            action: "save-choice",
            status: "loading",
            title: "Saving page…",
            cta: {
              label: "Screenshot instead",
              command: "save-screenshot",
              expiresAt: Date.now() + 3000,
            },
          },
          {},
          sendResponse,
        ),
      ).toBe(true);
      expect(sendResponse).not.toHaveBeenCalled();

      if (kind === "page") vi.advanceTimersByTime(3000);
      else
        document
          .getElementById(HOST_ID)!
          .shadowRoot!.querySelector<HTMLButtonElement>(".cta")!
          .click();
      expect(sendResponse).toHaveBeenCalledExactlyOnceWith({ kind });
    },
  );

  it("waits until the deadline to choose the default page save", () => {
    const onChoice = vi.fn();
    const host = offerSaveChoice(undefined, onChoice);
    const root = host.shadowRoot!;
    expect(root.querySelector(".cta")!.textContent).toBe("Screenshot instead");
    expect(root.querySelector(".cancel")).toBeNull();
    expect(root.querySelector<HTMLButtonElement>(".close")!.hidden).toBe(false);
    expect(root.querySelector(".countdown")).not.toBeNull();
    expect(onChoice).not.toHaveBeenCalled();

    vi.advanceTimersByTime(2999);
    expect(onChoice).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onChoice).toHaveBeenCalledExactlyOnceWith("page");
    expect(root.querySelector(".actions")).toBeNull();
    expect(root.querySelector(".countdown")).toBeNull();
    expect(root.querySelector<HTMLButtonElement>(".close")!.hidden).toBe(false);
    expect(root.querySelector(".spinner")).not.toBeNull();
    expect(document.getElementById(HOST_ID)).toBe(host);
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(60_000);
    expect(onChoice).toHaveBeenCalledTimes(1);
    expect(document.getElementById(HOST_ID)).toBe(host);
  });

  it("retains the supplied absolute deadline instead of starting a new three seconds", () => {
    const onChoice = vi.fn();
    const expiresAt = Date.now() + 3000;
    vi.advanceTimersByTime(1500);
    offerSaveChoice(expiresAt, onChoice);
    vi.advanceTimersByTime(1499);
    expect(onChoice).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onChoice).toHaveBeenCalledExactlyOnceWith("page");
  });

  it("chooses the default immediately when the supplied deadline already expired", () => {
    const onChoice = vi.fn();
    const host = offerSaveChoice(Date.now(), onChoice);
    expect(onChoice).toHaveBeenCalledExactlyOnceWith("page");
    expect(host.shadowRoot!.querySelector(".actions")).toBeNull();
    expect(host.shadowRoot!.querySelector(".countdown")).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("chooses screenshot only once and stops the default countdown", () => {
    const onChoice = vi.fn();
    const host = offerSaveChoice(undefined, onChoice);
    const button = host.shadowRoot!.querySelector<HTMLButtonElement>(".cta")!;
    button.click();
    button.click();

    expect(button.disabled).toBe(true);
    expect(onChoice).toHaveBeenCalledExactlyOnceWith("screenshot");
    expect(chrome.runtime.sendMessage).not.toHaveBeenCalled();
    expect(host.shadowRoot!.querySelector(".actions")).toBeNull();
    expect(host.shadowRoot!.querySelector(".countdown")).toBeNull();
    expect(host.shadowRoot!.querySelector(".title")!.textContent).toBe("Saving screenshot…");
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(5000);
    expect(onChoice).toHaveBeenCalledTimes(1);
    expect(document.getElementById(HOST_ID)).toBe(host);
  });

  it("chooses the page if a screenshot click reaches the handler after the deadline", () => {
    const onChoice = vi.fn();
    const host = offerSaveChoice(undefined, onChoice);
    vi.setSystemTime(Date.now() + 3000);
    host.shadowRoot!.querySelector<HTMLButtonElement>(".cta")!.click();
    expect(onChoice).toHaveBeenCalledExactlyOnceWith("page");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("dismisses the notification while leaving the default save scheduled", () => {
    const onChoice = vi.fn();
    const host = offerSaveChoice(undefined, onChoice);
    host.shadowRoot!.querySelector<HTMLButtonElement>(".close")!.click();
    expect(onChoice).not.toHaveBeenCalled();
    expect(document.getElementById(HOST_ID)).toBeNull();
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(3000);
    expect(onChoice).toHaveBeenCalledExactlyOnceWith("page");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cancels a dismissed pending choice when a newer save choice replaces it", () => {
    const onChoice = vi.fn();
    const host = offerSaveChoice(undefined, onChoice);
    host.shadowRoot!.querySelector<HTMLButtonElement>(".close")!.click();
    offerSaveChoice();
    expect(onChoice).toHaveBeenCalledExactlyOnceWith("cancel");
    vi.advanceTimersByTime(5000);
    expect(onChoice).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cancels a replaced pending choice without allowing its timer to affect the new choice", () => {
    const onChoice = vi.fn();
    offerSaveChoice(undefined, onChoice);
    vi.advanceTimersByTime(1000);
    const nextChoice = vi.fn();
    const host = offerSaveChoice(undefined, nextChoice);
    expect(onChoice).toHaveBeenCalledExactlyOnceWith("cancel");
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(2000);
    expect(document.getElementById(HOST_ID)).toBe(host);
    expect(onChoice).toHaveBeenCalledTimes(1);
    expect(nextChoice).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1000);
    expect(nextChoice).toHaveBeenCalledExactlyOnceWith("page");
  });

  it.each(["loading", "ok", "error"] as const)(
    "ignores an earlier save's %s toast during a visible pending choice",
    (status) => {
      const onChoice = vi.fn();
      const host = offerSaveChoice(undefined, onChoice);
      vi.advanceTimersByTime(1000);
      showToast({ action: "toast", status, title: "Earlier save update" });
      expect(onChoice).not.toHaveBeenCalled();
      expect(document.getElementById(HOST_ID)).toBe(host);
      expect(host.shadowRoot!.querySelector(".title")!.textContent).toBe("Saving page…");
      expect(host.shadowRoot!.querySelector(".actions")).not.toBeNull();
      vi.advanceTimersByTime(2000);
      expect(onChoice).toHaveBeenCalledExactlyOnceWith("page");
    },
  );

  it.each(["loading", "ok", "error"] as const)(
    "ignores an earlier save's %s toast during a dismissed pending choice",
    (status) => {
      const onChoice = vi.fn();
      const host = offerSaveChoice(undefined, onChoice);
      host.shadowRoot!.querySelector<HTMLButtonElement>(".close")!.click();
      vi.advanceTimersByTime(1000);
      showToast({ action: "toast", status, title: "Earlier save update" });
      expect(onChoice).not.toHaveBeenCalled();
      expect(document.getElementById(HOST_ID)).toBeNull();
      vi.advanceTimersByTime(2000);
      expect(onChoice).toHaveBeenCalledExactlyOnceWith("page");
    },
  );

  it("cleans up result dismissal timers when another save replaces the toast", () => {
    showToast({ action: "toast", status: "ok", title: "Saved" });
    vi.advanceTimersByTime(3200);
    expect(vi.getTimerCount()).toBe(1);
    const host = offerSaveChoice();
    vi.advanceTimersByTime(400);
    expect(document.getElementById(HOST_ID)).toBe(host);
    expect(host.shadowRoot!.querySelector(".actions")).not.toBeNull();
    vi.advanceTimersByTime(2600);
  });

  it("keeps the install action until it is used or dismissed", () => {
    showToast({
      action: "toast",
      status: "error",
      title: "Óia is not connected",
      cta: { label: "Connect Óia", command: "open-install" },
    });
    const host = document.getElementById(HOST_ID)!;
    vi.advanceTimersByTime(60_000);
    expect(document.getElementById(HOST_ID)).toBe(host);
    expect(host.shadowRoot!.querySelector(".countdown")).toBeNull();

    host.shadowRoot!.querySelector<HTMLButtonElement>(".cta")!.click();
    expect(chrome.runtime.sendMessage).toHaveBeenCalledExactlyOnceWith({
      action: "toast-cta",
      command: "open-install",
    });
    expect(document.getElementById(HOST_ID)).toBeNull();
  });
});
