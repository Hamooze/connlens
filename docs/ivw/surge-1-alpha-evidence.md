# Surge 1 Alpha Evidence

Date: 2026-08-06
Decision: review

## Implemented

- Tauri 2 + React + TypeScript + Tailwind app scaffold.
- `registry.json` v1 schema, stable ID hashing, missing lifecycle, removable semantics, backup recovery.
- Bundled provider descriptors for AWS, Azure, Cloudflare, Docker, GitHub, GitLab, Google Cloud, Vercel, Neon DB, Netlify, npm, Sentry, Shopify, Stripe, and Supabase candidates.
- Size-capped JSON/YAML/INI parser layer.
- GitHub multi-account parser, Vercel linked-project parser, token-file detection, env-var contract helpers.
- IPC commands for state, rescan, settings, remove, purge, reset, copy, dashboard URL validation, reveal source.
- Headless CLI read path: `list`, `status`, `providers`.
- Dark React popover UI with provider logos, popular-provider strip, grouped/searchable rows, expanded row actions, footer controls, settings view, X close, and titlebar drag.
- Passive footer scan status, dot-only watcher health, no filter button, no Show action, Delete row action, and custom-source provider form.
- Open Dashboard and Reveal Source actions validate and open their HTTPS URL or local source file target.
- Tray/work-area edge positioning for the popover before show.
- Sanitized fixtures and raw-secret grep script.
- Windows release binary and NSIS installer artifact.

## Verification Run

- `cd src-tauri; cargo test` - 17 tests passed.
- `cd src-tauri; cargo clippy -- -D warnings` - passed.
- `npm test` - 2 frontend helper tests passed.
- `npm run build` - production frontend build passed.
- `npm run tauri build` - release binary and NSIS bundle passed.
- `src-tauri\target\release\connlens.exe status` - returned clean stopped/headless status.
- Browser DOM/interaction/console checks - passed for footer, settings, Delete action visibility, custom-source submit, and no Filter/Show controls.
- Browser DOM/interaction/console check - passed for MCP absence and Vercel search visibility with zero console warnings/errors.
- Playwright smoke test - passed against compact and wide dark UI screenshots, dashboard popup, source toast, custom-source submit, and console-clean checks.
- Playwright screenshot fallback - captured `C:\Users\hamza\.codex\visualizations\2026\08\06\019fd670-aa30-7bd3-bd6d-c7fce14256c9\connlens-mcp-removed-vercel.png`.
- `src-tauri\target\release\connlens.exe status` - returned stopped/headless status with healthy watcher state.
- `src-tauri\target\release\connlens.exe providers --json` - returned the expanded bundled provider catalog without MCP/Claude descriptors.
- Source grep for `mcp-cursor`, `Cursor MCP`, `siCursor`, and `mcp_servers` in source/test files - no active catalog/scanner references found.
- `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1` - passed.
- Visual QA screenshots: `docs\ivw\visual-popover-390x520.png`, `docs\ivw\visual-popover-760x720.png`.

## Quality Gate Decision

Decision: review

Rationale: Core registry, parser, secret-handling, CLI read path, UI shell, visual snapshots, and packaging are implemented with automated evidence. Full Surge 1 cannot be marked pass because live file watchers/toasts/tray-position E2E, real Credential Manager enumeration, and Windows VM visual checks remain pending manual or follow-up implementation work.

## Required Follow-Up

- Implement `notify` watcher orchestration and real `state://updated` live flows.
- Replace Credential Manager stub with names-only Windows API enumeration.
- Run the Windows tray/DPI/VM IVW flows from `e2e/traceability.md`.
- Add CI jobs for cargo, npm, secret grep, SCA, and packaging.
