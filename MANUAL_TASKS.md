# Manual Tasks

## Blocking Before Descriptor Lock-In

- Required action: Verify real Windows CLI config paths for Vercel, Neon, Shopify, and later AWS/npm/Docker/SSH candidates from the source spec Section 13.
  Why Codex cannot complete it: It requires real installed CLIs and user-machine state beyond sanitized fixtures.
  Blocks future implementation: Yes, for final descriptor lock-in before GA.
  Resume step: Update provider descriptors and add sanitized fixtures from verified paths.

## Confirmatory Security Passes

- Required action: Verify real Git Credential Manager entries are enumerated as names/usernames only and no secret blob is read.
  Why Codex cannot complete it: It requires a real machine with GCM entries and reviewer observation.
  Blocks future implementation: No for alpha; yes before declaring GitHub detection complete for release.
  Resume step: Replace or extend `src-tauri/src/credman.rs` with names-only Windows API enumeration and attach redacted evidence.

## Windows UX Checks

- Required action: Test tray positioning on Windows 11 with 100% and 150% DPI, two monitors, and tray overflow flyout.
  Why Codex cannot complete it: Requires live shell/taskbar behavior.
  Blocks future implementation: No for code alpha; yes before alpha ship sign-off.
  Resume step: Record results in `docs/ivw/surge-1-alpha-evidence.md` and fix positioning if needed.

- Required action: Test Explorer reveal with paths containing spaces and non-ASCII characters, and Windows toast behavior with Focus Assist on/off.
  Why Codex cannot complete it: Requires interactive Windows shell behavior.
  Blocks future implementation: No for code alpha.
  Resume step: Add regression notes and update IPC action handling if needed.

## Release Blockers For Surge 2

- Required action: Acquire code-signing certificate and configure CI signing secrets.
  Why Codex cannot complete it: Requires business/legal account ownership and secret custody.
  Blocks future implementation: Yes, for signed GA installer.
  Resume step: Enable Tauri signing pipeline and run release checklist.

- Required action: Run SmartScreen, signed/portable build, reboot/autostart, and clean uninstall checks on a Windows VM.
  Why Codex cannot complete it: Requires a clean VM and manual reboot/security UI observation.
  Blocks future implementation: Yes, for GA release.
  Resume step: Complete `docs/release-checklist.md` in Surge 2.
