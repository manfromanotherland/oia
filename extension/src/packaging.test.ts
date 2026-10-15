// SPDX-License-Identifier: MIT

import { execFileSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("browser packaging", () => {
  it("ships working browser-specific backgrounds, identities, and only runtime files", () => {
    const root = process.cwd();
    const fixture = mkdtempSync(join(tmpdir(), "oia-extension-package-"));
    try {
      mkdirSync(join(fixture, "scripts"));
      cpSync(resolve(root, "scripts/package.mjs"), join(fixture, "scripts/package.mjs"));
      const source = JSON.parse(readFileSync(resolve(root, "manifest.json"), "utf8"));
      writeFileSync(join(fixture, "manifest.json"), JSON.stringify(source));
      mkdirSync(join(fixture, "dist"));
      for (const script of ["background", "content", "popup", "options", "page-video-main"]) {
        writeFileSync(join(fixture, `dist/${script}.js`), "/* bundled runtime */");
      }
      cpSync(resolve(root, "icons"), join(fixture, "icons"), { recursive: true });
      for (const page of ["options.html", "install.html"]) {
        cpSync(resolve(root, page), join(fixture, page));
      }
      writeFileSync(join(fixture, "popup.html"), "Do not ship the removed toolbar popup.");
      writeFileSync(join(fixture, "README.md"), "Do not ship project documentation.");
      writeFileSync(join(fixture, ".env"), "Do not ship environment files.");
      for (const directory of ["unpacked", "unpacked-firefox"]) {
        mkdirSync(join(fixture, directory));
        writeFileSync(join(fixture, directory, "stale.js"), "Remove old outputs.");
      }

      execFileSync(process.execPath, [join(fixture, "scripts/package.mjs")], {
        encoding: "utf8",
        timeout: 10_000,
      });

      const chromeArchive = join(fixture, `artifacts/oia-extension-chrome-${source.version}.zip`);
      const firefoxArchive = join(fixture, `artifacts/oia-extension-firefox-${source.version}.zip`);
      const archiveManifest = (archive: string) =>
        JSON.parse(execFileSync("unzip", ["-p", archive, "manifest.json"], { encoding: "utf8" }));
      const chrome = archiveManifest(chromeArchive);
      const firefox = archiveManifest(firefoxArchive);
      expect(chrome.manifest_version).toBe(3);
      expect(chrome.background).toEqual({ service_worker: "dist/background.js" });
      expect(chrome).not.toHaveProperty("browser_specific_settings");
      expect(chrome).not.toHaveProperty("key");
      expect(firefox.manifest_version).toBe(3);
      expect(firefox.background).toEqual({ scripts: ["dist/background.js"], persistent: false });
      expect(firefox.browser_specific_settings.gecko.id).toBe("cuttings@edmundo.is");
      expect(firefox.browser_specific_settings.gecko.strict_min_version).toBe("115.0");
      expect(firefox).not.toHaveProperty("key");
      expect(firefox).not.toHaveProperty("minimum_chrome_version");
      expect(firefox.permissions).toEqual(source.permissions);
      // Firefox 115 screenshot capture requires <all_urls>, not just activeTab.
      expect(firefox.host_permissions).toContain("<all_urls>");

      const development = JSON.parse(readFileSync(join(fixture, "unpacked/manifest.json"), "utf8"));
      expect(development.key).toBe(source.key);
      expect(readFileSync(join(fixture, `artifacts/oia-extension-${source.version}.zip`))).toEqual(
        readFileSync(chromeArchive),
      );
      for (const [archive, directory, manifest] of [
        [chromeArchive, "unpacked", chrome],
        [firefoxArchive, "unpacked-firefox", firefox],
      ] as const) {
        const files = execFileSync("unzip", ["-Z1", archive], { encoding: "utf8" }).split("\n");
        const background = manifest.background.service_worker ?? manifest.background.scripts[0];
        expect(files).toContain(background);
        expect(manifest.action).not.toHaveProperty("default_popup");
        expect(files).not.toContain("popup.html");
        expect(files).not.toContain("dist/popup.js");
        expect(files).toContain(manifest.options_ui.page);
        for (const icon of Object.values(manifest.icons)) expect(files).toContain(icon);
        expect(files).not.toContain("README.md");
        expect(files).not.toContain(".env");
        expect(files).not.toContain("stale.js");
        expect(files.some((file) => file.startsWith("scripts/"))).toBe(false);
        expect(existsSync(join(fixture, directory, "stale.js"))).toBe(false);
      }
    } finally {
      rmSync(fixture, { recursive: true, force: true });
    }
  });
});
