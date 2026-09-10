import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const bundle = resolve(process.argv[2] ?? "src-tauri/target/release/bundle");
const expectedIdentifier = "com.nemu.connlens";
const expectedPublisher = "Nemu";
const expectedCopyright = "Copyright © 2026 Nemu";
assert(existsSync(bundle), `Bundle directory missing: ${bundle}`);
function artifact(folder, suffix) {
  const directory = join(bundle, folder);
  assert(existsSync(directory), `Missing ${folder} bundle directory`);
  const matches = readdirSync(directory).filter((name) => name.endsWith(suffix));
  assert(matches.length > 0, `No ${suffix} bundle produced`);
  return matches.map((name) => join(directory, name));
}
function command(binary, args, env = {}) {
  const result = spawnSync(binary, args, { encoding: "utf8", timeout: 30_000, windowsHide: true, env: { ...process.env, ...env } });
  assert.equal(result.error, undefined, `${binary} could not run: ${result.error?.message}`);
  assert.equal(result.status, 0, `${binary} verification failed: ${result.stderr}`);
  return result.stdout;
}
function binaryHeader(path, expected) {
  assert(statSync(path).size > 1024, "Bundle is unexpectedly small");
  assert(readFileSync(path).subarray(0, expected.length).equals(Buffer.from(expected)), `Invalid bundle header: ${path}`);
}
function windowsVersionInfo(path) {
  // Read version resources without launching the executable or installer.
  // Keep the path out of PowerShell source so spaces and quoting stay literal.
  return JSON.parse(command("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", [
    "$ErrorActionPreference = 'Stop'",
    "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)",
    "$version = (Get-Item -LiteralPath $env:CONNLENS_BUNDLE_VERIFY_EXE).VersionInfo",
    "@{ CompanyName = $version.CompanyName; LegalCopyright = $version.LegalCopyright } | ConvertTo-Json -Compress",
  ].join("; ")], { CONNLENS_BUNDLE_VERIFY_EXE: path }));
}

if (process.platform === "darwin") {
  for (const app of artifact("macos", ".app")) {
    command("/usr/bin/codesign", ["--verify", "--deep", "--strict", app]);
    const plistPath = join(app, "Contents/Info.plist");
    command("/usr/bin/plutil", ["-lint", plistPath]);
    const plist = JSON.parse(command("/usr/bin/plutil", ["-convert", "json", "-o", "-", plistPath]));
    assert.equal(plist.CFBundleIdentifier, expectedIdentifier, "macOS bundle has the wrong owner identifier");
    assert.equal(plist.NSHumanReadableCopyright, expectedCopyright, "macOS bundle is missing Nemu copyright metadata");
    assert.equal(plist.LSUIElement, true, "macOS bundle must declare its menu-bar agent role at launch (LSUIElement)");
    const expected = process.argv[3] ?? (process.arch === "arm64" ? "arm64" : "x86_64");
    assert(command("/usr/bin/lipo", ["-archs", join(app, "Contents/MacOS/connlens")]).trim().split(/\s+/).includes(expected), "Wrong macOS binary architecture");
  }
  for (const dmg of artifact("dmg", ".dmg")) command("/usr/bin/hdiutil", ["verify", dmg]);
} else if (process.platform === "linux") {
  for (const deb of artifact("deb", ".deb")) {
    command("dpkg-deb", ["--info", deb]);
    assert.equal(command("dpkg-deb", ["--field", deb, "Maintainer"]).trim(), expectedPublisher, "Debian bundle is missing Nemu maintainer metadata");
  }
  for (const image of artifact("appimage", ".AppImage")) binaryHeader(image, [0x7f, 0x45, 0x4c, 0x46]);
} else if (process.platform === "win32") {
  const executable = resolve(bundle, "..", "connlens.exe");
  binaryHeader(executable, [0x4d, 0x5a]);
  const version = windowsVersionInfo(executable);
  assert.equal(version.CompanyName, expectedPublisher, "Windows application is missing Nemu company metadata");
  assert.equal(version.LegalCopyright, expectedCopyright, "Windows application is missing Nemu copyright metadata");
  for (const exe of artifact("nsis", ".exe")) {
    binaryHeader(exe, [0x4d, 0x5a]);
    // Tauri's NSIS template sets LegalCopyright, but not CompanyName.
    assert.equal(windowsVersionInfo(exe).LegalCopyright, expectedCopyright, "Windows installer is missing Nemu copyright metadata");
  }
} else {
  throw new Error(`Unsupported native bundle platform: ${process.platform}`);
}
console.log(`check_bundles: pass (${process.platform}/${process.arch})`);
