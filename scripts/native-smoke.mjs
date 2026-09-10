import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
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
  for (const id of ["github", "vercel", "neon", "mcp_servers"]) assert(catalog.some((provider) => provider.id === id), `Provider ${id} missing`);
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

  // Inject a duplicate of a known fixture record, including history worth
  // preserving. A scan must reconcile it without merging other identities.
  const duplicateRegistry = JSON.parse(cachedRegistry);
  const duplicateOriginal = duplicateRegistry.connections.find((row) => row.provider === "github" && row.source.sourceType === "config_file");
  assert(duplicateOriginal, "No GitHub fixture record available for reconciliation");
  const duplicateId = "fixture-legacy-duplicate-alias";
  duplicateRegistry.connections.push({
    ...structuredClone(duplicateOriginal), id: duplicateId,
    firstSeen: "2001-01-01T00:00:00.000Z", hidden: true, seen: false, status: "changed",
  });
  writeFileSync(registryPath, JSON.stringify(duplicateRegistry));
  const repaired = run(["list", "--json", "--all", "--rescan"]);
  assert.equal(repaired.connections.length, initial.connections.length, "Duplicate repair changed distinct fixture record count");
  assert(!repaired.connections.some((row) => row.id === duplicateId), "Legacy duplicate alias survived reconciliation");
  const repairedOriginal = repaired.connections.filter((row) => row.id === duplicateOriginal.id);
  assert.equal(repairedOriginal.length, 1, "Reconciliation lost the canonical record");
  assert.equal(repairedOriginal[0].firstSeen, "2001-01-01T00:00:00.000Z", "Reconciliation lost earliest history");
  assert.equal(repairedOriginal[0].hidden, true, "Reconciliation lost hidden preference");
  assert.equal(repairedOriginal[0].status, "changed", "Reconciliation lost an unseen change");

  // The file is only inspected for presence, never run. If the Unix fixture
  // were accidentally executed it would leave an unmistakable marker.
  const fixtureBin = join(fixtureHome, ".local/bin");
  mkdirSync(fixtureBin, { recursive: true });
  const ghExecutable = join(fixtureBin, process.platform === "win32" ? "gh.exe" : "gh");
  writeFileSync(ghExecutable, '#!/bin/sh\nprintf unexpected-execution > "$CONNLENS_HOME/cli-was-run"\nexit 97\n');
  if (process.platform !== "win32") chmodSync(ghExecutable, 0o700);
  const ghFound = run(["list", "--json", "--all", "--provider", "github", "--rescan"]);
  const observed = ghFound.connections.find((row) => row.id === duplicateOriginal.id);
  assert.equal(observed?.meta.toolPresence?.status, "found", "Fixture gh executable was not observed");
  const observedFile = statSync(observed.meta.toolPresence.path, { bigint: true });
  const expectedFile = statSync(ghExecutable, { bigint: true });
  assert.deepEqual([observedFile.dev, observedFile.ino], [expectedFile.dev, expectedFile.ino], "Executable check escaped the fixture candidate");
  assert.equal(observed.validation.availability, "available");
  rmSync(ghExecutable);
  const ghRemoved = run(["list", "--json", "--all", "--provider", "github", "--rescan"]);
  const accountAfterRemoval = ghRemoved.connections.find((row) => row.id === duplicateOriginal.id);
  assert.equal(accountAfterRemoval?.meta.toolPresence?.status, "missing", "Removal of an observed executable was not recorded");
  assert.equal(accountAfterRemoval.validation.availability, "available", "CLI removal incorrectly invalidated the saved account");
  assert.equal(accountAfterRemoval.removable, false, "CLI removal incorrectly authorized account history removal");
  assert(existsSync(accountAfterRemoval.source.path), "Executable check removed the account configuration");
  assert(!existsSync(join(fixtureHome, "cli-was-run")), "An executable presence check ran the fixture command");

  // Registration identity survives transport changes, read failures, actual
  // removal, and reappearance. No configured remote URL may be contacted.
  const mcpPath = join(fixtureHome, ".cursor/mcp.json");
  mkdirSync(dirname(mcpPath), { recursive: true });
  const mcpSecret = createHash("sha256").update("connlens-isolated-mcp-smoke").digest("hex");
  secrets.push(mcpSecret);
  writeFileSync(mcpPath, JSON.stringify({ mcpServers: {
    "fixture-server": { command: "node", args: [mcpSecret], env: { AUTH: mcpSecret } },
  } }));
  const readMcp = () => run(["list", "--json", "--all", "--provider", "mcp_servers", "--rescan"]).connections;
  const firstMcp = readMcp();
  assert.equal(firstMcp.length, 1, "MCP fixture registration was not inventoried");
  const mcpId = firstMcp[0].id;
  assert.equal(firstMcp[0].meta.mcpCommand, "node");
  assert.equal(firstMcp[0].meta.mcpTransport, "stdio");
  assert.equal(firstMcp[0].validation.availability, "available");
  assert.equal(firstMcp[0].removable, false);
  const remoteRegistration = { mcpServers: {
    "fixture-server": { type: "http", url: `https://fixture:${mcpSecret}@mcp.fixture.invalid/private?key=${mcpSecret}`, headers: { Authorization: mcpSecret } },
  } };
  writeFileSync(mcpPath, JSON.stringify(remoteRegistration));
  const remoteMcp = readMcp();
  assert.equal(remoteMcp.length, 1, "MCP transport update left a duplicate history entry");
  assert.equal(remoteMcp[0].id, mcpId, "MCP transport update changed registration identity");
  assert.equal(remoteMcp[0].status, "changed", "MCP transport update did not record a change");
  assert.notEqual(remoteMcp[0].fingerprint, firstMcp[0].fingerprint);
  assert.equal(remoteMcp[0].identity.host, "mcp.fixture.invalid");
  assert.equal(remoteMcp[0].meta.mcpTransport, "http");
  assert.equal(remoteMcp[0].meta.mcpCommand, undefined, "Remote registration retained a stale command");
  assert.equal(remoteMcp[0].meta.toolPresence, undefined, "Remote registration retained stale launcher evidence");
  assert(!JSON.stringify(remoteMcp).includes("/private?"), "MCP output exposed the remote URL path/query");
  writeFileSync(mcpPath, `{"mcpServers":{"fixture-server":{"url":"${mcpSecret}`);
  const unreadableMcp = readMcp();
  assert.equal(unreadableMcp.length, 1, "Malformed MCP config lost saved history");
  assert.equal(unreadableMcp[0].id, mcpId);
  assert.equal(unreadableMcp[0].validation.availability, "unknown", "Malformed MCP config was treated as absence");
  assert.equal(unreadableMcp[0].removable, false, "Malformed MCP config authorized history removal");
  writeFileSync(mcpPath, JSON.stringify({ mcpServers: {} }));
  const removedMcp = readMcp();
  assert.equal(removedMcp.length, 1);
  assert.equal(removedMcp[0].id, mcpId);
  assert.equal(removedMcp[0].validation.availability, "missing");
  assert.equal(removedMcp[0].removable, true, "Confirmed missing MCP registration was not removable");
  writeFileSync(mcpPath, JSON.stringify(remoteRegistration));
  const readdedMcp = readMcp();
  assert.equal(readdedMcp.length, 1, "Readded MCP registration duplicated history");
  assert.equal(readdedMcp[0].id, mcpId, "Readded MCP registration did not reuse its identity");
  assert.equal(readdedMcp[0].validation.availability, "available");
  assert.equal(readdedMcp[0].removable, false);
  rmSync(mcpPath);
  const deletedMcp = readMcp();
  assert.equal(deletedMcp[0].validation.reasonCode, "mcp_source_missing", "Deleted MCP config did not produce source absence evidence");

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
  console.log(`native_smoke: pass (${process.platform}/${process.arch}; catalog, scan, cache, duplicate repair, CLI presence/removal, MCP lifecycle, filter, rotation, missing history, redaction)`);
} finally {
  rmSync(fixtureHome, { recursive: true, force: true });
}
