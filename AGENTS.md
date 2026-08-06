# ConnLens Agent Guidance

- Keep generated app/runtime code inside `code-space/`.
- The target stack is Tauri 2, Rust, Vite, React, TypeScript, and Tailwind.
- Use Tauri native capabilities for tray/window management, opener, notifications, single-instance, and autostart.
- Use `notify` for file watching when live watchers are implemented; polling is only a fallback.
- Use `serde`, `serde_yaml`, `toml`, and `rust-ini` for standard config formats; do not hand-roll those parsers.
- Use `sha2` fingerprints and `scan::secutil::redact` for all secret-adjacent values.
- ConnLens must stay local-only: no product network calls, telemetry, external auth, or cloud sync.
- Honor `CONNLENS_HOME` in tests and local verification; never scan real user config in automated tests.
- Run `cargo test`, `npm test`, `npm run build`, and `scripts/secret_grep.ps1` before handoff when relevant.
