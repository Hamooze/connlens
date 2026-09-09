import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

assert(process.env.GITHUB_ACTIONS === "true", "Draft release helper only runs in GitHub Actions");
const version = JSON.parse(readFileSync("package.json", "utf8")).version;
assert(/^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(version), "Invalid package version");
const tag = process.env.GITHUB_REF_TYPE === "tag" ? process.env.GITHUB_REF_NAME : `v${version}`;
assert.equal(tag, `v${version}`, "Release tag must match package.json version");
const assets = [];
function collect(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) collect(path);
    else if (/\.(?:dmg|deb|AppImage|exe|app\.tar\.gz)$/.test(entry.name)) assets.push(path);
  }
}
collect(resolve(process.argv[2]));
assert.equal(assets.length, 7, "Expected 2 macOS DMGs, 2 app archives, Linux deb/AppImage, and Windows installer");
const run = (args) => spawnSync("gh", args, { encoding: "utf8", timeout: 120_000, stdio: ["ignore", "pipe", "pipe"] });
const existing = run(["release", "view", tag, "--json", "isDraft"]);
if (existing.status === 0) assert(JSON.parse(existing.stdout).isDraft, "Refusing to modify an already published release");
else assert(/release not found/i.test(existing.stderr ?? ""), `Could not safely inspect existing release: ${existing.stderr || existing.error?.message}`);
const scratch = mkdtempSync(join(tmpdir(), "connlens-release-"));
try {
  const notes = join(scratch, "notes.md");
  writeFileSync(notes, `ConnLens ${version}\n\nNative builds, fixture scan smoke tests, and bundle checks passed on the four build targets. WebKit and Chromium UI tests use mocked IPC.\n\nmacOS bundles are ad-hoc signed and are not notarized. Installation, OS login/autostart, system tray interactions, notifications, and Linux desktop compatibility require interactive platform acceptance before publishing.\n`);
  const action = existing.status === 0
    ? ["release", "edit", tag, "--draft", "--title", `ConnLens v${version}`, "--notes-file", notes]
    : ["release", "create", tag, "--draft", "--target", process.env.GITHUB_SHA, "--title", `ConnLens v${version}`, "--notes-file", notes];
  const result = run(action);
  assert.equal(result.status, 0, `Could not prepare draft release: ${result.stderr}`);
  const upload = run(["release", "upload", tag, ...assets, "--clobber"]);
  assert.equal(upload.status, 0, `Could not upload installers: ${upload.stderr}`);
  console.log(`Draft release ${tag} contains ${assets.length} verified installers/archives`);
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
