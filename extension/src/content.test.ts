// SPDX-License-Identifier: MIT

import { beforeEach, describe, expect, it } from "vitest";

import { showToast } from "./content.js";

const HOST_ID = "oia-toast-host";

describe("showToast", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
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
});
