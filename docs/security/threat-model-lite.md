# Threat Model Lite: ConnLens Surge 1

Date: 2026-08-06
Security tier: SEC-2

## Scope

ConnLens reads local developer app configuration files, fingerprints secret-like values, and writes metadata to the local user profile. It has no server, no login, no telemetry, and no product network calls.

## STRIDE Focus

- Tampering: user provider descriptors and future inbox files can be malformed or hostile.
- Information disclosure: registry, logs, errors, and UI details must not include raw secrets.
- Denial of service: oversized files and event storms must not block all providers.
- Elevation via execution: deferred to Surge 2 probe runner and must use a compiled allowlist.

## Hard Invariants

- Automated tests must run against `CONNLENS_HOME` fixtures, never real user data.
- Parser reads are capped at 1 MB.
- One provider failure does not stop other providers.
- Raw tokens are reduced to `sha256:xxxxxxxx` fingerprints before storage.
- Credential Manager support must enumerate names/usernames only.
- Dashboard URLs are HTTPS-only and descriptor-derived.

## Current Residual Risks

- Real Credential Manager enumeration is contract-stubbed and requires manual verification before release.
- File watchers are not fully implemented in this alpha; live update evidence is documented as pending.
- Installer signing and SmartScreen behavior are Surge 2 manual blockers.
