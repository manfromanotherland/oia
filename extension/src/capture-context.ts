// SPDX-License-Identifier: MIT

export const PAGE_CHANGED_MESSAGE = "The page changed while saving. Try again on the current page.";

/** Compare live browser documents, allowing fragment changes on the same page. */
export function isSamePage(expected: string | undefined, actual: string | undefined): boolean {
  try {
    if (!expected || !actual) return false;
    const left = new URL(expected);
    const right = new URL(actual);
    left.hash = "";
    right.hash = "";
    return left.href === right.href;
  } catch {
    return false;
  }
}

export function isWebPage(value: string | undefined): boolean {
  try {
    const url = new URL(value ?? "");
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

export class PageChangedError extends Error {
  constructor() {
    super(PAGE_CHANGED_MESSAGE);
    this.name = "PageChangedError";
  }
}
