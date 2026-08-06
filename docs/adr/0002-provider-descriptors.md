# ADR 0002: TOML Provider Descriptors

Status: Accepted
Date: 2026-08-06

Provider locations and strategies are defined in bundled TOML files under `src-tauri/resources/providers/`, with user overrides planned under `CONNLENS_HOME/providers/`.

This keeps path changes data-driven and preserves the Surge 2 contract for Tier B providers and probe metadata.
