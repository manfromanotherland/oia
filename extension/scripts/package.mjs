// SPDX-License-Identifier: MIT
// Packages the built extension for Chromium and Firefox.
//
// Both `npm run build` and its `npm run package` alias bundle first, then invoke
// this script. It stages only what the manifest references — no src/, node_modules/, or signing
// keys — into stable browser-specific unpacked directories and ZIP archives.
//
// Chromium retains the source key only in the development directory. Firefox
// uses a Manifest V3 event page and retains its native-messaging add-on ID.

import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// Everything the packaged extension loads at runtime. Anything not listed here
// (src/, node_modules/, package files, *.pem) stays out of the zip.
const CONTENTS = [
  "manifest.json",
  "dist/background.js",
  "dist/content.js",
  "dist/options.js",
  "dist/page-video-main.js",
  "icons",
  "options.html",
  "install.html",
];

function fail(message) {
  console.error(`✗ ${message}`);
  process.exit(1);
}

// `zip` ships with macOS but not every environment; fail with a clear hint.
try {
  execFileSync("zip", ["--version"], { stdio: "ignore" });
} catch {
  fail(
    "`zip` was not found on PATH. It is preinstalled on macOS; on Debian/Ubuntu run `sudo apt-get install zip`.",
  );
}

const missing = CONTENTS.filter((entry) => !existsSync(join(root, entry)));
if (missing.length > 0) {
  fail(
    `missing build output: ${missing.join(", ")}. Run \`npm run build\` first (\`npm run package\` does this for you).`,
  );
}

const sourceManifest = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));
const { version } = sourceManifest;

const outDir = join(root, "artifacts");
mkdirSync(outDir, { recursive: true });

function stageRuntime(directory, manifest) {
  rmSync(directory, { recursive: true, force: true });
  mkdirSync(directory, { recursive: true });
  for (const entry of CONTENTS) {
    if (entry !== "manifest.json") {
      mkdirSync(dirname(join(directory, entry)), { recursive: true });
      cpSync(join(root, entry), join(directory, entry), { recursive: true });
    }
  }
  writeFileSync(join(directory, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
}

for (const browser of ["chrome", "firefox"]) {
  const manifest = structuredClone(sourceManifest);
  if (browser === "firefox") {
    delete manifest.key;
    delete manifest.minimum_chrome_version;
    manifest.background = { scripts: ["dist/background.js"], persistent: false };
  } else {
    delete manifest.browser_specific_settings;
    manifest.background = { service_worker: "dist/background.js" };
  }

  const unpackedDir = join(root, browser === "chrome" ? "unpacked" : "unpacked-firefox");
  stageRuntime(unpackedDir, manifest);

  const stageDir = join(outDir, `stage-${browser}`);
  delete manifest.key;
  stageRuntime(stageDir, manifest);
  const outFile = join(outDir, `oia-extension-${browser}-${version}.zip`);
  rmSync(outFile, { force: true });
  try {
    // Relative archive paths; no macOS attributes or Finder metadata.
    execFileSync("zip", ["-r", "-X", "-q", outFile, ...CONTENTS, "-x", "*.DS_Store"], {
      cwd: stageDir,
      stdio: "inherit",
    });
  } finally {
    rmSync(stageDir, { recursive: true, force: true });
  }

  console.log(`✓ ${browser} v${version} → ${unpackedDir}`);
  console.log(`✓ Archive → ${outFile}`);
}

// Keep the existing archive path as a Chromium alias for local release scripts.
cpSync(
  join(outDir, `oia-extension-chrome-${version}.zip`),
  join(outDir, `oia-extension-${version}.zip`),
);
