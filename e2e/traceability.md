# Surge 1 Traceability Matrix

| Flow ID | Flow Description | Source Waves | Evidence | Status |
|---------|------------------|--------------|----------|--------|
| S1-F1 | Fixture scan renders provider groups and rows within 5 seconds | 1.1.1-1.1.5, 1.2.2 | `tests/fixtures/home`, pending Tauri visual run | PARTIAL |
| S1-F2 | GitHub multi-account fixture shows all identities with active marker | 1.1.3 | `cargo test scan::strategies::tests::gh_multi_account_shape_yields_active_flags` | AUTOMATED |
| S1-F3 | Vercel linked project from `.vercel/project.json` appears without raw project/org IDs | 1.1.4 | `cargo test scan::strategies::tests::vercel_project_links_are_fingerprinted` | AUTOMATED |
| S1-F4 | Search narrows by provider/label/host and clear restores | 1.2.2 | `npm test` connection view helpers | AUTOMATED |
| S1-F5 | Expand row, copy, open dashboard, reveal source, delete | 1.2.3 | React UI wired to IPC/dev action wrappers; pending Tauri visual run | PARTIAL |
| S1-F6 | Retired MCP rows are hidden/pruned and active auto-detected rows remain remove-blocked | 1.2.3, 1.1.1 | `cargo test registry::tests::retired_provider_rows_are_hidden_and_pruned` and `cargo test registry::tests::active_auto_detected_rows_are_not_removable` | AUTOMATED |
| S1-F7 | Live append emits row/toast/dot within 2 seconds | 1.3.1, 1.3.2 | Not implemented in this alpha | MANUAL/PENDING |
| S1-F8 | Deleted Vercel auth becomes missing and persists after restart | 1.1.5, 1.1.1 | registry missing lifecycle test; pending fixture E2E | PARTIAL |
| S1-F9 | Pause watchers prevents updates and resume rescans | 1.3.1 | Settings/menu state stub only | PENDING |
| S1-F10 | Provider error shows inline notice; Retry rescans | 1.2.3 | dev fallback provider notice; pending Tauri E2E | PARTIAL |
| S1-F11 | Restart persists history/collapse and second launch focuses | 1.1.1, 1.2.1 | registry tests; single-instance configured, pending shell E2E | PARTIAL |
| S1-F12 | Secret grep over registry/logs finds zero fixture tokens | 1.1.3 | `scripts/secret_grep.ps1` | AUTOMATED |
