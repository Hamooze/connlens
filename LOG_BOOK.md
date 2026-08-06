# Log Book

Project: Connlens

## 2026-08-06 12:40 - Repo start scaffold

Prompt: Start the project with the repo-start structure.
Change: Created `prototypes/`, `code-space/`, `externals/`, and the initial tracking files.
Files touched: `HANDOFF.md`, `LOG_BOOK.md`, `AUDIT_BOOK.md`
Verification: Scaffold script completed.
Notes: Initial scaffold used the existing `C:\Users\hamza\Downloads\Connlens` folder by passing its parent directory as `--root`.

## 2026-08-06 12:40 - GitHub remote created

Prompt: Use repo-start for the Connlens workspace.
Change: Created private GitHub repository `Hamooze/connlens`, attached it as `origin`, and updated tracking docs with the current remote/state.
Files touched: `HANDOFF.md`, `LOG_BOOK.md`, `AUDIT_BOOK.md`
Verification: `gh repo create` completed and returned `https://github.com/Hamooze/connlens`.
Notes: First push is intended for branch `codex/connlens-start`.

## 2026-08-06 14:01 - ConnLens alpha foundation build

Prompt: Build from `C:\Users\hamza\Downloads\connlens-surge-plan_1.md` using `orchstate-full-build`.
Change: Added a Tauri 2 + React + TypeScript ConnLens alpha with provider descriptors, local registry, parsers, fixture-backed scanners, Tauri IPC commands, headless CLI read commands, compact popover UI, security docs, traceability docs, visual evidence, and NSIS packaging.
Files touched: `package.json`, `src\`, `src-tauri\`, `tests\`, `docs\`, `e2e\`, `scripts\`, `README.md`, `AGENTS.md`, `MANUAL_TASKS.md`
Verification: `npm test`, `npm run build`, `cd src-tauri; cargo test`, `cd src-tauri; cargo clippy -- -D warnings`, `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1`, release CLI `status`, `list --json --all`, and `providers --json`.
Artifacts: `src-tauri\target\release\connlens.exe`; `src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe`; screenshots in `docs\ivw\visual-popover-390x520.png` and `docs\ivw\visual-popover-760x720.png`.
Notes: Gate is alpha foundation `review`; full Surge 1 pass remains blocked by real watcher/toast/tray-position E2E, Credential Manager enumeration, and Windows VM/manual checks.

## 2026-08-06 14:44 - Dark utility UI and provider expansion

Prompt: Make the app default dark mode, add popular developer-app logos/providers, remove Hide, add X/drag/tray-position behavior, remove useless Claude Code MCP, and fix Vercel/Neon visibility.
Change: Reworked the popover into a compact dark desktop utility UI, added provider logos and a popular-provider rail, removed the user-facing Hide action, added titlebar drag and X-close-to-tray behavior, positioned the popover near the tray/work-area edge, expanded bundled providers, corrected Vercel and Neon DB descriptor paths, and removed Claude-specific MCP descriptors.
Files touched: `src\App.tsx`, `src\App.css`, `src\lib\api.ts`, `src\lib\store.ts`, `src-tauri\src\`, `src-tauri\resources\providers\`, `src-tauri\capabilities\default.json`, `src-tauri\tauri.conf.json`, `tests\`, `package.json`, `docs\ivw\`, `README.md`, `HANDOFF.md`, `AUDIT_BOOK.md`
Verification: `npm test`, `npm run build`, `cd src-tauri; cargo test`, `cd src-tauri; cargo clippy -- -D warnings`, `npm run tauri build`, Playwright smoke test, release CLI `status`, release CLI `providers --json`, and `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1`.
Artifacts: `src-tauri\target\release\connlens.exe`; `src-tauri\target\release\bundle\nsis\ConnLens_0.1.0_x64-setup.exe`; refreshed screenshots in `docs\ivw\visual-popover-390x520.png` and `docs\ivw\visual-popover-760x720.png`.
Notes: The Browser skill's in-app browser attach timed out during visual QA, so the smoke test used local Playwright instead.

## 2026-08-06 15:12 - Footer, settings, and row action cleanup

Prompt: Make scan status a passive footer look, make watcher healthy just a green thing, remove the filter button, replace settings providers with custom providers, remove Show, add Delete, and make Reveal Source/Open Dashboard open targets.
Change: Removed the filter button and Show action, changed footer scan status to passive text, changed watcher health to a dot-only indicator, replaced settings Provider List with a Custom Sources form that writes user descriptors, added a Delete action to expanded rows, and changed Tauri dashboard/source commands to open HTTPS links and local source files through the opener plugin.
Files touched: `src\App.tsx`, `src\App.css`, `src\lib\api.ts`, `src\lib\store.ts`, `src\lib\types.ts`, `src-tauri\src\commands.rs`, `src-tauri\src\lib.rs`, `README.md`, `HANDOFF.md`, `AUDIT_BOOK.md`
Verification: `npm test`, `npm run build`, `cd src-tauri; cargo test`, `cd src-tauri; cargo clippy -- -D warnings`, `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1`, Browser DOM/interaction/console checks, and Playwright screenshot smoke.
Artifacts: Screenshots in `C:\Users\hamza\.codex\visualizations\2026\08\06\019fd670-aa30-7bd3-bd6d-c7fce14256c9\connlens-request-list-390.png`, `connlens-request-settings-390.png`, and `connlens-request-wide-760.png`.
Notes: Browser interaction validation worked this pass; its screenshot command was unavailable, so local Playwright was used for screenshot evidence.

## 2026-08-06 15:24 - GitHub publish prep

Prompt: Push live to GitHub and update README/handoff files.
Change: Added GitHub branch/publish details to `README.md` and `HANDOFF.md`, kept generated Playwright `test-results` out of version control, and prepared the current `codex/connlens-start` branch for push to `Hamooze/connlens`.
Files touched: `.gitignore`, `README.md`, `HANDOFF.md`, `LOG_BOOK.md`, `AUDIT_BOOK.md`
Verification: `gh auth status` confirmed `Hamooze` is authenticated and active; final source/test verification from the previous pass remains current.
Notes: Branch-first workflow retained; no merge to `main` was performed.

## 2026-08-06 15:27 - MCP retirement and Vercel project links

Prompt: Remove the MCP section because Claude/Cursor MCP does not count, and fix Vercel systems not showing.
Change: Removed the bundled Cursor MCP descriptor, retired MCP/Claude provider IDs in descriptor loading, saved-state snapshots, and scan pruning, removed MCP fixtures/UI catalog entries, and added Vercel `.vercel/project.json` linked-project detection with fingerprint-only project/org IDs.
Files touched: `src\App.tsx`, `src-tauri\src\`, `src-tauri\resources\providers\`, `tests\fixtures\`, `README.md`, `HANDOFF.md`, `AUDIT_BOOK.md`, `LOG_BOOK.md`, `docs\`, `e2e\traceability.md`, `MANUAL_TASKS.md`
Verification: `npm test`, `npm run build`, `cd src-tauri; cargo test`, `cd src-tauri; cargo clippy -- -D warnings`, `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1`, `npm run tauri build`, release CLI `status`, release CLI `providers --json`, Browser DOM/search/console smoke, Playwright screenshot fallback, and source grep for removed MCP catalog entries.
Notes: Vercel CLI project linking is detected from local `.vercel/project.json` files; raw project/org IDs are not persisted.

## 2026-08-06 15:39 - Icon-only footer rescan

Prompt: Make the footer Rescan control show only the loop icon, not the word.
Change: Removed visible Rescan text from the footer button, kept aria-label/title, and fixed footer control columns to icon-button widths.
Files touched: `src\App.tsx`, `src\App.css`, `LOG_BOOK.md`, `AUDIT_BOOK.md`
Verification: `npm test`, `npm run build`, `npm run tauri build`, Browser DOM/interaction/console smoke, and Playwright screenshot capture.

## 2026-08-06 15:45 - Dark scrollbar polish

Prompt: Remove the visible provider rail slider while keeping it scrollable, and make the right scrollbar a white bar on black background.
Change: Hid the horizontal provider rail scrollbar, removed its bottom scrollbar padding, and styled content/settings vertical scrollbars with a black track and white thumb.
Files touched: `src\App.css`, `LOG_BOOK.md`, `AUDIT_BOOK.md`
Verification: `npm test`, `npm run build`, `npm run tauri build`, Browser DOM/interaction/console scroll-style smoke, and Playwright screenshot capture.

## 2026-08-06 15:52 - Nemu settings footer

Prompt: Add "Powered by Nemu.ae" under Settings with a pressable nemu.ae link and the newest Nemu logo from the local Nemu folder.
Change: Copied the Nemu brand-kit SVG into public assets, added a compact Settings footer, and routed the nemu.ae link through a validated Tauri external URL opener.
Files touched: `public\nemu-logo.svg`, `src\App.tsx`, `src\App.css`, `src\lib\api.ts`, `src-tauri\src\commands.rs`, `src-tauri\src\lib.rs`, `LOG_BOOK.md`, `AUDIT_BOOK.md`
Verification: `npm test`, `npm run build`, `cd src-tauri; cargo test`, `cd src-tauri; cargo clippy -- -D warnings`, `powershell -ExecutionPolicy Bypass -File scripts\secret_grep.ps1`, Browser DOM/interaction/console footer check, Playwright screenshot capture, and `npm run tauri build`.
