# Handoff

Last updated: 2026-08-16 06:14 +04:00
Project: Connlens
Theme: ConnLens dark utility alpha
Current branch: main
GitHub remote: `https://github.com/Hamooze/connlens`
Publish target: `origin/main` with `origin/codex/connlens-start` mirrored for continuity
Run command: `npm run dev`
Build command: `npm run build`
Desktop build command: `npm run tauri build`
Platform build commands: `npm run tauri:build:windows`, `npm run tauri:build:linux`, `npm run tauri:build:macos`
Test commands: `npm test`, `cd src-tauri; cargo test`, `cd src-tauri; cargo clippy -- -D warnings`, `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1`

## Current State

ConnLens is a runnable Tauri 2 + React + TypeScript alpha foundation for local developer app connection visibility. It now defaults to a compact dark utility UI, opens near the tray/work-area edge, supports titlebar dragging, uses an X button to hide the popover back to tray, exposes a horizontally scrollable provider icon rail, and includes a Settings Start on startup toggle backed by the Tauri autostart plugin. The app includes registry persistence, expanded provider descriptors, a custom-source provider form, parsers, fixture-backed provider strategies, headless CLI read/rescan commands, documentation, Windows local packaging, and a cross-platform GitHub Actions release workflow for Linux and macOS builds.

Release binary: `src-tauri\target\release\connlens.exe`
Installer: `src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe`
Linux/macOS artifacts: produced by `.github/workflows/desktop-release.yml` on native GitHub runners.

## Latest Push

- Latest change: prepared the branch for GitHub publication and main merge with Windows/Linux/macOS build scripts/workflow, Azure profile detection cleanup, visible top provider-rail horizontal scrolling, updated README/HANDOFF, Windows CI `ripgrep` installation, and a current `macos-15-intel` hosted-runner label for the Intel macOS release job.
- Release app was last relaunched from `src-tauri\target\release\connlens.exe` as PID `30452`; recheck PID live before assuming it is still running.
- Latest focused checks: `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings`, `npm run build`, `npm test`, `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1`, `npm run tauri:build:windows`.

## Major Updates

- Implemented the ConnLens alpha app shell from the surge plan.
- Added registry v1 state, stable connection IDs, missing/changed/hidden lifecycle, backup recovery, provider descriptors, parsers, and sanitized fixtures.
- Added GitHub, Vercel, Neon, Shopify, AWS, Azure, Google Cloud, Docker, npm, GitLab, Netlify, Cloudflare, Stripe, Supabase, and Sentry candidate scanning contracts with fingerprint-only secret handling.
- Added Tauri IPC commands, tray/menu scaffolding, release CLI commands, and the React popover/settings UI.
- Reworked the UI into the dark compact desktop-tool style from the requested reference, including provider logos, a popular-provider rail, draggable titlebar, and X close button.
- Replaced the default Tauri app icon with a ConnLens share-mark icon generated from `src-tauri\icon-source.svg`.
- Added `scripts\register_start_menu_shortcut.ps1` and `npm run register:start-menu` to register repo-run release builds in the Windows Start Menu.
- Removed the user-facing Hide feature and retired MCP/Claude provider IDs so existing saved rows are hidden and pruned on scan.
- Added account-label auto-detection across token/config/profile rows; Vercel linked-project scanning remains limited to explicitly configured project roots only, while regular access rows prefer connected user, team, org, tenant, AWS SSO account, role, or account labels when available.
- Added `user_id`/`userId` account-label fallback and automatic cleanup for stale Vercel linked-project rows plus superseded fingerprint fallback rows from the same source file.
- Replaced the settings Provider List with Custom Sources, removed the filter button, made watcher health dot-only, made scan status passive footer text, added a Delete row action, and made dashboard/source actions open the target.
- Added a Settings Start on startup toggle and enabled the tray Launch at startup menu item against the same native autostart state.
- Moved the settings save status into the bottom settings footer and changed the label from `Saved` to `Save`.
- Added cross-platform bundle targets, Linux/macOS-friendly path expansion fallbacks, process-env scanning on non-Windows, platform-specific package build scripts, and a native-runner GitHub Actions desktop release workflow.
- Added Windows CI scan-tool installation so the desktop release workflow can run `scripts\secret_grep.ps1` on hosted Windows runners.
- Updated the Intel macOS release job from the stale `macos-13` runner label to `macos-15-intel`.
- Fixed Azure CLI account detection for UTF-8-BOM `azureProfile.json` files, removed the generic Azure config fallback row, pruned stale generic Azure rows when a real account profile is detected, and added `connlens list --rescan` for explicit CLI refreshes.
- Reverted the visible scrollbar styling for the main detected-items list while keeping the settings scrollbar styling, then restored a visible horizontal scrollbar for the top provider icon rail.
- Added docs, ADRs, security notes, traceability, manual tasks, and visual evidence under `docs\ivw`.

## Important Decisions

- Only `code-space/` is the Git working tree.
- Work stays branch-first unless the user explicitly asks to merge or push to main.
- Secret values are never persisted; only short SHA-256 fingerprints are stored.
- macOS CI artifacts are ad-hoc signed by default; Developer ID signing and notarization remain a separate distribution step.
- Hidden records remain in the registry schema only for backward compatibility; the current UI/API no longer exposes Hide.
- Credential Manager enumeration is currently a names-only follow-up boundary, not a blob-reading implementation.

## Next Steps

- Complete real Windows Credential Manager names-only enumeration.
- Add notify watcher orchestration and full toast/tray-position E2E flows.
- Run the Windows VM/manual IVW checklist in `MANUAL_TASKS.md`.
- Watch the latest desktop release workflow on GitHub and verify draft-release artifacts after all native runners finish.
- Add Apple Developer ID signing/notarization secrets if ConnLens needs polished public macOS distribution.
- Add SCA/security packaging checks.

## Known Risks

- Quality gate remains `review`, not `pass`, because real watchers/toasts/tray-position E2E, Credential Manager enumeration, Windows VM reboot/autostart checks, and native Linux/macOS runner artifact smoke tests are pending.
