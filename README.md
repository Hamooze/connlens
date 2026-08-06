# ConnLens

ConnLens is a local-only Windows tray utility for seeing developer app connections on a developer machine. It defaults to a compact dark popover, scans local configuration files, stores metadata in `%LOCALAPPDATA%\ConnLens\registry.json` or `CONNLENS_HOME`, and never persists raw secret values.

GitHub: `https://github.com/Hamooze/connlens`
Current development branch: `codex/connlens-start`

## Providers

Bundled alpha providers: AWS, Azure, Cloudflare, Docker, GitHub, GitLab, Google Cloud, Neon DB, Netlify, npm, Sentry, Shopify, Stripe, Supabase, and Vercel.

Access-token and profile detection prefer connected account labels such as email, username, org/team, AWS SSO account name, AWS account/role, tenant, or account ID when those fields are present. Project/resource IDs stay as context instead of primary row labels unless the provider descriptor uses an explicit project-link or configured project-root location, such as `$PROJECT_ROOTS/.vercel/project.json`. MCP and Claude/Cursor MCP sources are retired from the built-in catalog; use Custom Sources only for unusual non-built-in local files that should be treated as ordinary token/config providers.

The UI includes brand marks for the common developer apps, a popular-provider strip, search/grouping, custom sources, a draggable titlebar, and an X button that hides the popover back to tray. The previous Hide/Show action is intentionally removed.

Settings includes a Start on startup toggle backed by the Tauri autostart plugin. Custom sources can be added from Settings with a name, optional ID, optional HTTPS dashboard URL, config paths, env vars, and parser format. Custom descriptors are saved under the ConnLens app data `providers` folder.

## Commands

```powershell
npm install
npm run dev
npm test
npm run build
npm run tauri dev
npm run tauri build
cd src-tauri; cargo test
powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1
```

Windows installer artifact:

```powershell
src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe
```

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

- Local-only: no network calls, no accounts, no telemetry.
- Secrets are fingerprinted with `sha256:xxxxxxxx`; raw fixture token grep is enforced by `scripts/secret_grep.ps1`.
- Credential Manager support is names/usernames only in the current contract; real-machine verification remains a manual task.
