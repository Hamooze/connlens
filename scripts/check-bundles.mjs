import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const bundle = resolve(process.argv[2] ?? "src-tauri/target/release/bundle");
assert(existsSync(bundle), `Bundle directory missing: ${bundle}`);
function artifact(folder, suffix) {
  const directory = join(bundle, folder);
  assert(existsSync(directory), `Missing ${folder} bundle directory`);
  const matches = readdirSync(directory).filter((name) => name.endsWith(suffix));
  assert(matches.length > 0, `No ${suffix} bundle produced`);
  return matches.map((name) => join(directory, name));
}
function command(binary, args) {
  const result = spawnSync(binary, args, { encoding: "utf8", timeout: 30_000 });
  assert.equal(result.status, 0, `${binary} verification failed: ${result.stderr}`);
  return result.stdout;
}
function binaryHeader(path, expected) {
  assert(statSync(path).size > 1024, "Bundle is unexpectedly small");
  assert(readFileSync(path).subarray(0, expected.length).equals(Buffer.from(expected)), `Invalid bundle header: ${path}`);
}

if (process.platform === "darwin") {
  for (const app of artifact("macos", ".app")) {
    command("/usr/bin/codesign", ["--verify", "--deep", "--strict", app]);
    command("/usr/bin/plutil", ["-lint", join(app, "Contents/Info.plist")]);
    const expected = process.argv[3] ?? (process.arch === "arm64" ? "arm64" : "x86_64");
    assert(command("/usr/bin/lipo", ["-archs", join(app, "Contents/MacOS/connlens")]).trim().split(/\s+/).includes(expected), "Wrong macOS binary architecture");
  }
  for (const dmg of artifact("dmg", ".dmg")) command("/usr/bin/hdiutil", ["verify", dmg]);
} else if (process.platform === "linux") {
  for (const deb of artifact("deb", ".deb")) command("dpkg-deb", ["--info", deb]);
  for (const image of artifact("appimage", ".AppImage")) binaryHeader(image, [0x7f, 0x45, 0x4c, 0x46]);
} else if (process.platform === "win32") {
  for (const exe of artifact("nsis", ".exe")) binaryHeader(exe, [0x4d, 0x5a]);
} else {
  throw new Error(`Unsupported native bundle platform: ${process.platform}`);
}
console.log(`check_bundles: pass (${process.platform}/${process.arch})`);
