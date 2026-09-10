# ConnLens

ConnLens is developed and owned by [Nemu](https://nemu.ae). Its application identifier is `com.nemu.connlens`, and its installer publisher is Nemu. Copyright © 2026 Nemu.

ConnLens is a local-only Tauri 2 utility for inspecting developer app connections found in local configuration. It uses a compact tray/menu-bar panel with provider filters, searchable accounts, expandable redacted details, Settings, and Quit. On macOS the panel has a top pointer, monochrome icons, a blue selected tab, dark inset cards, and a two-button footer. Closing the panel hides it; Quit exits the application. On macOS, the bundle declares its menu-bar agent role at launch so window-management utilities can recognize it as a background app. Reopening ConnLens from Applications or Spotlight restores the existing panel.

The application does not make provider API requests, collect telemetry, authenticate to external services, or sync data. Explicit “Open dashboard” and website links open the system browser. Available status means a local reference exists; it does not prove that a credential is accepted by a provider.

## Providers and data

Bundled account providers: AWS, Azure, Cloudflare, Docker, GitHub, GitLab, Google Cloud, Neon DB, Netlify, npm, Sentry, Shopify, Stripe, Supabase, and Vercel. **MCP servers** is a separate inventory of local client registrations. Account labels use email, username, account/profile, org/team, or tenant information when available in local files. Raw secret values are never intentionally persisted; fingerprints and redacted metadata are displayed instead.

Each scan consolidates saved duplicates only when their account, profile, host, and source establish the same record. This includes older IDs for the same record. It preserves history dates, hidden state, metadata, and unseen changes. Matching labels or credentials alone do not merge different accounts or sources. Consolidation updates ConnLens's registry; it does not edit provider files or uninstall software. MCP registrations have stable identities based on their source file, client, and server key, so changing a registration's URL updates that record.

Settings controls file watching, fallback scanning, notifications, and start at login. Changes to process environment values require a rescan or application restart; they are not file-watch events. Custom providers accept local config paths or environment-variable names and standard JSON, YAML, INI, or TOML formats. Custom descriptors are stored under the app-data `providers` directory. Credential Manager enumeration is currently a stub, including on Windows. Discovery uses supported local configuration files and environment values; vault-only accounts are not detected.

`CONNLENS_HOME` activates fixture isolation as well as selecting the app-data directory. Standard user/config locations resolve inside this directory; inherited credential environment variables, OS credential stores, and external custom paths are excluded. Startup registration and notifications are also isolated, and separate fixture homes use separate runtime instances. Automated checks must always use this override.

When upgrading from the former `com.brdg.connlens` identifier, quit the old app before launching the Nemu build. The default macOS data folder moves from `com.brdg.ConnLens` to `com.nemu.ConnLens`, preserving history, settings, and custom providers. An existing Nemu data folder is never overwritten. Normal Windows and Linux data paths remain unchanged, as do login startup entries. Windows users who previously chose a custom installation folder may need to select that folder again in the installer.

## CLI and MCP evidence

Account availability and executable presence are separate checks. Expanded rows show **CLI found**, **CLI removed**, **CLI not found**, or **CLI unchecked** for supported provider commands:

- **Found** means an executable exists in a checked location. It has not been run.
- **Removed** means an executable previously observed by ConnLens is now absent, and the checked replacement locations were searched completely.
- **Not found** means no executable was located; without earlier evidence this does not establish uninstallation.
- **Unchecked** means the command or its search locations could not be checked reliably.

A saved account can remain available after its CLI is removed. Executable evidence never makes an otherwise available or unknown account eligible for history cleanup. These checks do not inspect running processes, test authentication, or run package managers. The app's environment and checked locations can differ from an interactive shell.

The MCP inventory reads these supported sources:

| Client | Local source | Registration map |
| --- | --- | --- |
| Codex | `~/.codex/config.toml` and `$CODEX_HOME/config.toml` when set | `mcp_servers` |
| Claude Desktop | The platform app-config folder's `Claude/claude_desktop_config.json` (`~/Library/Application Support` on macOS; `%APPDATA%` on Windows) | `mcpServers` |
| Claude Code | `~/.claude.json`; `.mcp.json` in configured project roots | `mcpServers` |
| Cursor | `~/.cursor/mcp.json`; `.cursor/mcp.json` in configured project roots | `mcpServers` |
| VS Code | `.vscode/mcp.json` in configured project roots | `servers` |

Configured stdio and HTTP/SSE registrations are inventoried, including disabled registrations. Presence means a registration is saved locally; it does not establish that the MCP server is installed, running, enabled, or reachable. Stdio rows can separately show **Launcher** executable evidence. Finding `npx`, `uvx`, `docker`, or another launcher does not establish that its package, container, or server is available. Remote endpoints are never contacted.

Only the executable field, client, server name, transport, disabled flag, and redacted URL hostname are displayed. Arguments, environment values, authentication headers, and URL credentials, paths, queries, and fragments are not exposed as MCP metadata; changes to the registration are detected through a SHA-256 fingerprint. The local configuration file path is retained as the source.

Reads are limited to regular files of at most 1 MB, 256 registrations per source, and the first 50 configured project roots. JSON sources currently require strict JSON: comments/JSONC and nested `~/.claude.json.projects[path].mcpServers` mappings are not inventoried. Unsupported or malformed registration shapes protect historical entries as unknown. A successfully read configuration with no corresponding named registration, or a confirmed absent source file, can establish that the saved registration is missing locally. The new `mcp_servers` inventory does not restore retired legacy `mcp-*` or `claude-*` records.

## Validation and cleanup

Use the check-list button above Connections or **Settings → Review cleanup** to scan local sources and review saved history. Each entry shows availability, usage evidence, the last check time, and the reason for its classification:

- **Available locally**: the scanner found the local reference.
- **Missing locally**: a supported, enabled source was successfully checked and the historical reference is absent.
- **Could not check**: a source is unreadable, unsupported, outside current scan scope, incomplete, has no verifiable local source, or belongs to a disabled provider. These entries remain protected. A disabled MCP registration that is still present is reported as available with an explicit disabled explanation.

**Selected in local config** means the provider explicitly identifies that account as selected. **Referenced locally** means a reference exists without proof of selection. Neither proves recent runtime use or that a provider accepts the credentials; validation makes no network calls.

Only confirmed-missing entries are selected for cleanup. Review removal from an individual row preselects only that entry. Removal clears ConnLens history, and execution checks the current sources again under the registry lock. Reviews expire after five minutes and can be used once. Reappearing accounts and changed sources are retained with an explanation.

Removing a missing MCP entry clears its saved ConnLens history. MCP client and project configuration files remain protected, including files that also contain settings or other registrations. This action does not remove an existing server registration from a client.

**Include leftover files** adds a separate, initially unchecked file selection. This inventory covers only files associated with ConnLens entries, not applications, caches, or unrelated disk files. Eligible files must be empty after parsing, dedicated to a supported single-account configuration, owned by the current user where the OS exposes ownership, and inside the user folder. Every associated entry must be selected and still missing. Shared, project, meaningful/nonempty, oversized, unreadable, linked, and ConnLens internal files are protected. File content and identity are checked against the review immediately before the native Trash operation.

macOS uses native Trash; Windows uses a recycle-only Recycle Bin operation; Linux uses desktop Trash. There is no permanent-delete fallback. A failed file move keeps the associated history, and results identify retained entries and file failures. Files can be recovered through the operating system's Trash. Automated tests use an injected temporary fixture mover; the shipped `CONNLENS_HOME` boundary refuses desktop Trash operations.

## Develop and validate

Install the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/), Node.js 24, and stable Rust, then run these commands from this repository (`code-space/` in the local project wrapper):

```sh
npm ci
npm test
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run test:secrets
npx playwright install chromium webkit
npm run test:e2e
```

On Windows, also run the required PowerShell entry point:

```powershell
./scripts/secret_grep.ps1
```

The PowerShell script and `npm run test:secrets` invoke the same portable check. Failures print filenames without exposing matching secrets. Browser screenshots/traces are saved outside the repository under the system temporary `connlens-playwright` folder, or `CONNLENS_E2E_OUTPUT` when set.

`npm run dev` is a browser preview with synthetic data. It does not scan the computer. Production assets require Tauri; browser tests inject a test-only IPC implementation.

To verify the real native CLI without reading personal configuration:

```sh
cargo build --locked --manifest-path src-tauri/Cargo.toml
npm run test:smoke
# Or pass a release executable explicitly:
node scripts/native-smoke.mjs src-tauri/target/release/connlens
```

On Windows, the executable ends in `.exe`. The smoke runner creates and cleans a temporary fixture home, removes inherited credentials from the child environment, and checks the provider catalog, native scan, cached reads, duplicate repair, observed executable removal, the MCP registration lifecycle, provider filtering, credential rotation, missing history, and absence of raw fixture secrets from CLI output and persisted state.

For interactive fixture-only native development on macOS/Linux:

```sh
export CONNLENS_HOME="$(mktemp -d)"
cp -R tests/fixtures/home/. "$CONNLENS_HOME/"
npm run tauri dev
# After quitting the app:
rm -rf "$CONNLENS_HOME"
unset CONNLENS_HOME
```

Use a new temporary directory for each run. Do not point tests at a personal home/config directory or the checked-in fixtures themselves.

## Platform build matrix

Native installers must be built on the matching operating system. The [Desktop validation workflow](.github/workflows/desktop-ci.yml) runs on pull requests, pushes to `main`, manual dispatch, and release validation. Its configured targets are:

| Build target | Native validation runner | Bundles |
| --- | --- | --- |
| macOS Apple Silicon | `macos-15`, ARM64 | `.app` archive, `.dmg` |
| macOS Intel | `macos-15-intel`, x64 | `.app` archive, `.dmg` |
| Linux x64 | Ubuntu 22.04 | `.deb`, `.AppImage` |
| Windows x64 | Windows Server 2022 | NSIS `.exe` |

Each native job runs unit tests, builds installers, exercises the real executable against isolated fixtures, and checks bundle structure and Nemu ownership metadata. macOS checks additionally verify ad-hoc signatures, architecture, and DMG integrity. A separate browser job exercises the production UI with WebKit at 360×520 and 390×520, and Chromium at 390×520, using mocked native commands for the respective platform presentation. Browser tests cover filtering/search, details/copy, reviewed history removal, explicit file selection, protected sources, stale reviews and partial results, settings persistence, watcher events, custom-provider validation/retry, scan errors, and Quit command dispatch.

These checks provide separate evidence for UI behavior, native scanning, and packaging. They do not establish installation or native desktop behavior on every OS version. The workflow must run successfully for the current commit before its target builds can be called verified. Interactive acceptance remains necessary for tray placement/reopening, multiple monitors/scaling, OS clipboard and source reveal, autostart after login, system notifications, install/uninstall, and Linux desktop environments. Windows needs WebView2; Linux compatibility depends on WebKitGTK and the distribution libraries. The Ubuntu 22.04 build baseline follows [Tauri's AppImage compatibility guidance](https://v2.tauri.app/distribute/appimage/).

On Linux, use the tray context menu to reopen ConnLens: [Tauri does not emit tray mouse events on Linux](https://v2.tauri.app/learn/system-tray/). The desktop must support an AppIndicator/system tray, and X11 versus Wayland positioning and focus require validation on the intended desktop. A headless Linux build does not verify those behaviors.

Local bundle commands:

```sh
npm run tauri:build:macos
npm run tauri:build:windows
npm run tauri:build:linux
node scripts/check-bundles.mjs src-tauri/target/release/bundle
```

Only run the command for the host OS. Explicit macOS targets place outputs under `src-tauri/target/<target-triple>/release/bundle/`; otherwise bundles are under `src-tauri/target/release/bundle/`. Windows ARM64 and Linux ARM64 are not in the current release matrix. macOS artifacts are ad-hoc signed and are not notarized; Windows code signing is not configured.

## Resource use

ConnLens uses native file events with compiled filters for enabled provider paths, auxiliary identity files, MCP configuration files, executable candidates, custom descriptors, and the inbox. The watch plan includes absent candidate files and resolved executable targets, allowing creation, removal, and supported launcher changes to trigger a fresh check. Unrelated writes do not schedule a scan. Watch registrations are refreshed when directories, descriptors, launcher targets, saved tool paths, or scan settings change; timed scans are reserved for watcher failures. Pausing watchers keeps the settings file observable so watching can resume. Missing optional system executable directories are not watched through broad system ancestors; use Rescan after creating those locations. Changes to the parent shell's environment or PATH are not file events and may require restarting ConnLens.

The desktop async runtime uses at most two workers. Hidden popovers skip snapshot rendering and receive current state when reopened. Account rows subscribe only to their own expansion state, so messages and loading changes do not redraw the list. Development sample data is excluded from production bundles. The scanner reuses immutable provider templates and redaction patterns, reads only configured environment variables, and avoids copying full credential objects; raw credentials are never retained in these caches.

Measure release builds with an isolated `CONNLENS_HOME`. Distinguish native process memory from WebKit/WebView memory and compare the same workload over repeated runs. Thread counts and avoided scans establish reduced work; they do not establish a fixed percentage of RAM or battery savings.

## Releases

The [Desktop release workflow](.github/workflows/desktop-release.yml) runs on manual dispatch or a `v*` tag. It first completes the entire validation workflow, then gathers the same verified artifacts into a draft release. Tags must match `package.json` (`v0.1.0`, for example). It refuses to alter a published release. Creating a draft does not publish it or replace the remaining interactive platform acceptance checks.

For an uninstalled Windows development build, `npm run register:start-menu` adds a Start menu shortcut pointing to `src-tauri/target/release/connlens.exe`.

## Agent read contract

The executable also provides a local read interface:

```sh
connlens list --json
connlens list --json --rescan
connlens list --provider github --all
connlens status
connlens providers --json
```

`list --json` returns `{ "schemaVersion": 1, "connections": [] }`. It reads persisted metadata without rescanning unless `--rescan` is supplied. Each connection includes `validation` with `availability`, `usage`, `checkedAt`, `reason`, and `reasonCode`. Optional `meta.toolPresence` provides separate executable evidence; MCP registrations use provider ID `mcp_servers` and redacted MCP metadata. Missing entries are retained in history and included with `--all`; older entries without validation are treated as unknown until checked. Scanning requires only local access; authentication against provider services is outside this application's behavior.
