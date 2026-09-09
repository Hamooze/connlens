import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const binary = resolve(process.argv[2] ?? join(root, "src-tauri/target/debug", process.platform === "win32" ? "connlens.exe" : "connlens"));
assert(existsSync(binary), `Build the native executable first; missing ${binary}`);
const fixtureHome = realpathSync(mkdtempSync(join(tmpdir(), "connlens-smoke-")));
const secrets = readFileSync(join(root, "tests/fixtures/raw-secret-values.txt"), "utf8").split(/\r?\n/).filter((value) => value.trim());
// Inherited credentials and config overrides never reach the executable.
const systemKeys = new Set(["PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT", "TEMP", "TMP", "TMPDIR", "LANG", "LC_ALL"]);
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => systemKeys.has(key.toUpperCase())));
Object.assign(env, {
  CONNLENS_HOME: fixtureHome,
  HOME: fixtureHome,
  USERPROFILE: fixtureHome,
  APPDATA: join(fixtureHome, "appdata"),
  LOCALAPPDATA: join(fixtureHome, "localappdata"),
  XDG_CONFIG_HOME: join(fixtureHome, ".config"),
  XDG_DATA_HOME: join(fixtureHome, ".local/share"),
});

function secretFree(text, description) {
  assert(!secrets.some((secret) => text.includes(secret)), `Raw fixture secret found in ${description}`);
}

function run(args, json = true) {
  const result = spawnSync(binary, args, { env, cwd: fixtureHome, encoding: "utf8", timeout: 30_000, maxBuffer: 4 * 1024 * 1024, windowsHide: true });
  secretFree(result.stdout ?? "", "CLI stdout");
  secretFree(result.stderr ?? "", "CLI stderr");
  assert.equal(result.error, undefined, `Native command failed to start: ${result.error?.message}`);
  assert.equal(result.status, 0, `Native command ${args[0]} failed (exit ${result.status})`);
  return json ? JSON.parse(result.stdout) : result.stdout;
}

try {
  cpSync(join(root, "tests/fixtures/home"), fixtureHome, { recursive: true });
  const catalog = run(["providers", "--json"]);
  for (const id of ["github", "vercel", "neon"]) assert(catalog.some((provider) => provider.id === id), `Provider ${id} missing`);
  assert.equal(new Set(catalog.map((provider) => provider.id)).size, catalog.length, "Duplicate catalog providers");
  const initial = run(["list", "--json", "--rescan"]);
  assert.equal(initial.schemaVersion, 1);
  for (const provider of ["github", "vercel", "neon"]) {
    assert(initial.connections.some((row) => row.provider === provider), `Fixture scan missed ${provider}`);
  }
  assert(initial.connections.every((row) => ["github", "vercel", "neon"].includes(row.provider)), "Scan escaped fixture provider set");
  const registryPath = join(fixtureHome, "registry.json");
  const cachedRegistry = readFileSync(registryPath, "utf8");
  const cached = run(["list", "--json"]);
  assert.deepEqual(cached.connections, initial.connections, "Cached read changed the snapshot");
  assert.equal(readFileSync(registryPath, "utf8"), cachedRegistry, "Cached read wrote registry state");
  assert.match(run(["status"], false), /last_scan=(?!never)/);

  const github = run(["list", "--json", "--provider", "github"]);
  assert(github.connections.length > 0 && github.connections.every((row) => row.provider === "github"), "Provider filter failed");
  const sourcePaths = [...new Set(github.connections.filter((row) => row.source.sourceType === "config_file").map((row) => row.source.path))];
  assert(sourcePaths.length > 0, "GitHub fixture config was not read");
  for (const path of sourcePaths) {
    const local = relative(fixtureHome, realpathSync(path));
    assert(local && !local.startsWith("..") && !isAbsolute(local), "Source path escaped fixture home");
    const original = readFileSync(path, "utf8");
    const changed = original.replace(/oauth_token:\s*([^\s]+)/g, (_, token) => {
      const replacement = createHash("sha256").update(`rotated-${token}`).digest("hex");
      secrets.push(replacement);
      return `oauth_token: ${replacement}`;
    });
    assert.notEqual(changed, original, "No GitHub fixture token to rotate");
    writeFileSync(path, changed);
  }
  const rotated = run(["list", "--json", "--provider", "github", "--rescan"]);
  assert(rotated.connections.some((row) => row.status === "changed"), "Credential rotation did not mark a changed connection");
  for (const row of rotated.connections) {
    if (row.fingerprint) assert.match(row.fingerprint, /^sha256:[a-f0-9]+$/, "Unredacted fingerprint format");
  }
  for (const path of sourcePaths) rmSync(path);
  assert.equal(run(["list", "--json", "--provider", "github", "--rescan"]).connections.length, 0, "Missing rows visible without --all");
  const missing = run(["list", "--json", "--provider", "github", "--all"]);
  assert(missing.connections.length > 0 && missing.connections.every((row) => row.status === "missing" && row.removable), "Missing history was not preserved");
  assert(run(["list", "--json", "--provider", "neon"]).connections.length > 0, "Scoped scan removed other providers");
  for (const filename of readdirSync(fixtureHome).filter((name) => name.startsWith("registry."))) {
    secretFree(readFileSync(join(fixtureHome, filename), "utf8"), filename);
  }
  console.log(`native_smoke: pass (${process.platform}/${process.arch}; catalog, scan, cache, filter, rotation, missing history, redaction)`);
} finally {
  rmSync(fixtureHome, { recursive: true, force: true });
}
