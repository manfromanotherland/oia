// SPDX-License-Identifier: MIT

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("extension manifest", () => {
  it("installs the selection context listener in embedded frames", () => {
    const manifest = JSON.parse(readFileSync(resolve(process.cwd(), "manifest.json"), "utf8")) as {
      content_scripts: Array<{ js: string[]; all_frames?: boolean }>;
    };
    expect(
      manifest.content_scripts.find((script) => script.js.includes("dist/content.js"))?.all_frames,
    ).toBe(true);
  });

  it("uses only the MV3 service worker background declaration", () => {
    const manifest = JSON.parse(readFileSync(resolve(process.cwd(), "manifest.json"), "utf8")) as {
      manifest_version: number;
      background: Record<string, unknown>;
    };

    expect(manifest.manifest_version).toBe(3);
    expect(manifest.background).toEqual({ service_worker: "dist/background.js" });
  });
});
