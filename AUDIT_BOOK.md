# Audit Book

Project: Connlens
Last audit: 2026-08-06 15:27 +03:00
Current branch: codex/connlens-start
Remote: https://github.com/Hamooze/connlens

## Current Code State

Runnable ConnLens alpha foundation in `code-space/`: Tauri 2 desktop app, dark React/TypeScript popover UI, local registry, descriptor-driven developer-app scanners, fixture corpus, CLI read commands, security/traceability docs, and Windows release packaging.

## Last Code Changed

Retired MCP/Claude provider IDs from the catalog, scanner, UI, fixtures, and saved-state snapshots; added Vercel `.vercel/project.json` linked-project detection with fingerprint-only project/org metadata.

## Verification

- `npm test` passed: 1 file, 2 tests.
- `npm run build` passed.
- `cd src-tauri; cargo test` passed: 17 tests.
- `cd src-tauri; cargo clippy -- -D warnings` passed.
- `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1` passed.
- `npm run tauri build` passed and emitted the release binary plus NSIS installer.
- Browser DOM/interaction/console checks passed for footer, settings, Delete, and custom-source flows.
- Browser DOM/interaction/console check passed for MCP removal and Vercel search visibility with no console warnings/errors.
- Playwright smoke test passed against compact and wide dark UI screenshots, dashboard popup, source toast, and custom-source submit with no console warnings.
- Playwright screenshot fallback captured `C:\Users\hamza\.codex\visualizations\2026\08\06\019fd670-aa30-7bd3-bd6d-c7fce14256c9\connlens-mcp-removed-vercel.png`.
- `src-tauri\target\release\connlens.exe status` returned `running=no` and `watcher_health=Ok`.
- `src-tauri\target\release\connlens.exe providers --json` returned AWS, Azure, Cloudflare, Docker, Google Cloud, GitHub, GitLab, Neon DB, Netlify, npm, Sentry, Shopify, Stripe, Supabase, and Vercel, with no MCP/Cursor provider.
- Visual QA screenshots captured at 390x520 and 760x720; sampled pixel checks were nonblank.

## Rollback Point

Previous remote checkpoint: `2a1e760 Initialize repo-start workspace`.

## Open Risks

- Gate remains `review`, not full Surge 1 `pass`.
- Windows Credential Manager enumeration is still a names-only follow-up stub.
- Live file watcher orchestration, toasts, and tray-position/DPI E2E need follow-up manual or automated validation.
- Installer is unsigned; SmartScreen, reboot, and autostart flows require Windows VM validation.

## Next Audit Focus

- Validate real Windows account sources and manual IVW flows from `MANUAL_TASKS.md`.
- Add CI for Rust, frontend, secret grep, SCA, and packaging.
