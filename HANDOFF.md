# Handoff

Last updated: 2026-08-07 01:44 +03:00
Project: Connlens
Theme: ConnLens dark utility alpha
Current branch: codex/connlens-start
GitHub remote: `https://github.com/Hamooze/connlens`
Publish target: `origin/codex/connlens-start`
Run command: `npm run dev`
Build command: `npm run build`
Desktop build command: `npm run tauri build`
Test commands: `npm test`, `cd src-tauri; cargo test`, `cd src-tauri; cargo clippy -- -D warnings`, `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1`

## Current State

ConnLens is a runnable Tauri 2 + React + TypeScript alpha foundation for local developer app connection visibility. It now defaults to a compact dark utility UI, opens near the tray/work-area edge, supports titlebar dragging, uses an X button to hide the popover back to tray, and exposes a Settings Start on startup toggle backed by the Tauri autostart plugin. The app includes registry persistence, expanded provider descriptors, a custom-source provider form, parsers, fixture-backed provider strategies, headless CLI read commands, documentation, and a Windows NSIS installer artifact.

Release binary: `src-tauri\target\release\connlens.exe`
Installer: `src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe`

## Major Updates

- Implemented the ConnLens alpha app shell from the surge plan.
- Added registry v1 state, stable connection IDs, missing/changed/hidden lifecycle, backup recovery, provider descriptors, parsers, and sanitized fixtures.
- Added GitHub, Vercel, Neon, Shopify, AWS, Azure, Google Cloud, Docker, npm, GitLab, Netlify, Cloudflare, Stripe, Supabase, and Sentry candidate scanning contracts with fingerprint-only secret handling.
- Added Tauri IPC commands, tray/menu scaffolding, release CLI commands, and the React popover/settings UI.
- Reworked the UI into the dark compact desktop-tool style from the requested reference, including provider logos, a popular-provider rail, draggable titlebar, and X close button.
- Removed the user-facing Hide feature and retired MCP/Claude provider IDs so existing saved rows are hidden and pruned on scan.
- Added Vercel linked-project scanning for `.vercel/project.json` with project/org IDs fingerprinted before storage.
- Replaced the settings Provider List with Custom Sources, removed the filter button, made watcher health dot-only, made scan status passive footer text, added a Delete row action, and made dashboard/source actions open the target.
- Added a Settings Start on startup toggle and enabled the tray Launch at startup menu item against the same native autostart state.
- Added docs, ADRs, security notes, traceability, manual tasks, and visual evidence under `docs\ivw`.

## Important Decisions

- Only `code-space/` is the Git working tree.
- Work stays branch-first unless the user explicitly asks to merge or push to main.
- Secret values are never persisted; only short SHA-256 fingerprints are stored.
- Hidden records remain in the registry schema only for backward compatibility; the current UI/API no longer exposes Hide.
- Credential Manager enumeration is currently a names-only follow-up boundary, not a blob-reading implementation.

## Next Steps

- Complete real Windows Credential Manager names-only enumeration.
- Add notify watcher orchestration and full toast/tray-position E2E flows.
- Run the Windows VM/manual IVW checklist in `MANUAL_TASKS.md`.
- Add CI jobs for cargo, npm, secret grep, SCA, and packaging.

## Known Risks

- Quality gate remains `review`, not `pass`, because real watchers/toasts/tray-position E2E, Credential Manager enumeration, and Windows VM reboot/autostart checks are pending.
