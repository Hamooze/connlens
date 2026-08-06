# Audit Book

Project: Connlens
Last audit: 2026-08-07 01:39 +03:00
Current branch: codex/connlens-start
Remote: https://github.com/Hamooze/connlens

## Current Code State

Runnable ConnLens alpha foundation in `code-space/`: Tauri 2 desktop app, dark React/TypeScript popover UI, local registry, descriptor-driven developer-app scanners, fixture corpus, CLI read commands, security/traceability docs, and Windows release packaging.

## Last Code Changed

Switched the Settings footer to the provided Nemu PNG logo beside the pressable `https://nemu.ae` link.

## Verification

- `npm test` passed: 1 file, 2 tests.
- `npm run build` passed.
- Browser DOM/interaction/console check passed for the provided Nemu PNG footer logo: the image loaded from `/nemulogo_withouttxt.png` at 28x28, the `https://nemu.ae` link remained actionable, and there were no console warnings/errors.
- Playwright screenshot captured `C:\Users\hamza\.codex\visualizations\2026\08\06\019fd670-aa30-7bd3-bd6d-c7fce14256c9\connlens-nemu-provided-png-footer.png`.
- `npm run tauri build` passed and refreshed `src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe`.
- Browser DOM/interaction/console check passed for the alternate Nemu footer logo: the image loaded from `/nemu-mark-luminous.svg` at 20x20, the `https://nemu.ae` link remained actionable, and there were no console warnings/errors.
- Playwright screenshot captured `C:\Users\hamza\.codex\visualizations\2026\08\06\019fd670-aa30-7bd3-bd6d-c7fce14256c9\connlens-nemu-mark-footer.png`.
- `npm run tauri build` passed and refreshed `src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe`.
- Browser DOM/interaction/console check passed for the Nemu settings footer: logo loaded from `/nemu-logo.svg`, the `https://nemu.ae` link was actionable by trial click, and there were no console warnings/errors.
- Playwright screenshot captured `C:\Users\hamza\.codex\visualizations\2026\08\06\019fd670-aa30-7bd3-bd6d-c7fce14256c9\connlens-nemu-settings-footer.png`.
- `npm run tauri build` passed and refreshed `src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe`.
- Browser DOM/interaction/console check passed for hidden provider rail scrollbar, preserved horizontal rail scrolling, and white-on-black vertical scrollbar styles.
- Playwright screenshot captured `C:\Users\hamza\.codex\visualizations\2026\08\06\019fd670-aa30-7bd3-bd6d-c7fce14256c9\connlens-hidden-horizontal-scrollbar.png`.
- `npm run tauri build` passed and refreshed `src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe`.
- Browser DOM/interaction/console check passed for icon-only footer Rescan control; the button retained title/aria access and updated scan time without visible footer text.
- Playwright screenshot captured `C:\Users\hamza\.codex\visualizations\2026\08\06\019fd670-aa30-7bd3-bd6d-c7fce14256c9\connlens-icon-only-rescan.png`.
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
