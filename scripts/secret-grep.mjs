import { readFileSync, readdirSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Shared by the PowerShell entry point and non-Windows validation.
const root = resolve(process.argv[2] ?? resolve(dirname(fileURLToPath(import.meta.url)), ".."));
const secretsPath = resolve(root, process.argv[3] ?? "tests/fixtures/raw-secret-values.txt");
const secrets = readFileSync(secretsPath, "utf8").split(/\r?\n/).filter((value) => value.trim()).map((value) => Buffer.from(value));
const ignoredDirectories = new Set([".git", ".local", "node_modules", "dist", "dist-ssr", "test-results", "playwright-report"]);
const failures = new Set();

function scan(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    const local = relative(root, path).replaceAll("\\", "/");
    if (entry.isSymbolicLink()) continue;
    if (entry.isDirectory()) {
      if (!ignoredDirectories.has(entry.name) && local !== "tests/fixtures" && local !== "src-tauri/target") scan(path);
    } else if (entry.isFile() && entry.name !== "package-lock.json") {
      const contents = readFileSync(path);
      if (secrets.some((secret) => contents.includes(secret))) failures.add(local);
    }
  }
}

scan(root);
if (failures.size) {
  // Never include matched secret values or matching source lines in logs.
  console.error(`Raw fixture secrets found outside allowed fixtures in:\n${[...failures].sort().join("\n")}`);
  process.exitCode = 1;
} else {
  console.log("secret_grep: pass");
}
