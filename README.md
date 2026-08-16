# ConnLens

ConnLens is a local-first desktop tray/menu utility for seeing developer app connections on a developer machine. It supports Windows, Linux, and macOS through Tauri, defaults to a compact dark popover, scans local configuration files, stores metadata in the platform app-data directory or `CONNLENS_HOME`, and never persists raw secret values.

GitHub: `https://github.com/Hamooze/connlens`
Current development branch: `main`

## Providers

Bundled alpha providers: AWS, Azure, Cloudflare, Docker, GitHub, GitLab, Google Cloud, Neon DB, Netlify, npm, Sentry, Shopify, Stripe, Supabase, and Vercel.

Access-token and profile detection prefer connected account labels such as email, username, user ID, org/team, AWS SSO account name, AWS account/role, tenant, or account ID when those fields are present. Project/resource IDs stay as context instead of primary row labels unless the provider descriptor uses an explicit project-link or configured project-root location, such as `$PROJECT_ROOTS/.vercel/project.json`. Missing project-link rows, generic Azure config rows, and superseded fingerprint fallback rows are pruned automatically on rescan so stale project history does not bury the connected account/profile rows. Azure CLI profiles with a UTF-8 BOM are accepted. MCP and Claude/Cursor MCP sources are retired from the built-in catalog; use Custom Sources only for unusual non-built-in local files that should be treated as ordinary token/config providers.

The UI includes brand marks for the common developer apps, a horizontally scrollable popular-provider strip, search/grouping, custom sources, a draggable titlebar, and an X button that hides the popover back to tray. The previous Hide/Show action is intentionally removed.

Settings includes a Start on startup toggle backed by the Tauri autostart plugin. Custom sources can be added from Settings with a name, optional ID, optional HTTPS dashboard URL, config paths, env vars, and parser format. Custom descriptors are saved under the ConnLens app data `providers` folder.

## Commands

```powershell
npm install
npm run dev
npm test
npm run build
npm run tauri dev
npm run tauri build
npm run tauri:build:windows
npm run tauri:build:linux
npm run tauri:build:macos
npm run register:start-menu
cd src-tauri; cargo test
powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1
```

Native bundle commands must run on the matching OS. Windows builds NSIS, Linux builds Debian/AppImage bundles, and macOS builds `.app`/`.dmg` bundles with ad-hoc signing by default.

Windows installer artifact:

```powershell
src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe
```

Linux artifacts are written under `src-tauri/target/release/bundle/deb/` and `src-tauri/target/release/bundle/appimage/`.

macOS artifacts are written under `src-tauri/target/<target-triple>/release/bundle/macos/` and `src-tauri/target/<target-triple>/release/bundle/dmg/` when using an explicit macOS target.

## Cross-Platform Releases

`.github/workflows/desktop-release.yml` builds ConnLens on native GitHub runners and is the supported way to produce non-Windows artifacts:

- Windows x64: NSIS installer
- Linux x64: `.deb` and AppImage
- macOS Apple Silicon: `.app` and `.dmg`
- macOS Intel: `.app` and `.dmg`

Run it from GitHub Actions with `workflow_dispatch`, or push a `v*` tag. The workflow creates/updates a draft GitHub release named `ConnLens v__VERSION__`. macOS Apple Silicon uses the current default macOS ARM runner, and macOS Intel uses the current hosted Intel label `macos-15-intel`. macOS artifacts are ad-hoc signed unless Apple Developer signing and notarization secrets are added later.

For local repo-run builds that are not installed through NSIS, `npm run register:start-menu` creates `%APPDATA%\Microsoft\Windows\Start Menu\Programs\ConnLens.lnk` pointing at `src-tauri\target\release\connlens.exe`. This makes ConnLens appear under Windows Search Apps instead of only finding the project folder.

Fixture scan setup for local verification:

```powershell
$env:CONNLENS_HOME = "$PWD\tests\fixtures\home"
$env:APPDATA = "$PWD\tests\fixtures\home\appdata"
$env:USERPROFILE = "$PWD\tests\fixtures\home"
npm run tauri dev
```

## Agent Read Contract

The low-cost Surge 2 read path is available through the same executable:

```powershell
connlens list --json
connlens list --json --rescan
connlens list --provider github --all
connlens status
connlens providers --json
```

`list --json` returns:

```json
{
  "schemaVersion": 1,
  "connections": []
}
```

Mutation, named-pipe, inbox, advanced probes, reboot/autostart validation, and signed installer work are tracked as later Surge 2 items in `MANUAL_TASKS.md` and `docs/ivw/surge-1-alpha-evidence.md`.

## Security Posture

- Local-first: scans local files and environment data by default, with no telemetry or ConnLens account.
- Optional probe enrichment can call provider APIs, currently used to resolve Vercel account/team display names when Probe enrichment is enabled.
- Secrets are fingerprinted with `sha256:xxxxxxxx`; raw fixture token grep is enforced by `scripts/secret_grep.ps1`.
- Credential Manager support is names/usernames only in the current contract; real-machine verification remains a manual task.
