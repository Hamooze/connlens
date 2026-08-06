# ADR 0003: Fingerprint-Only Secret Handling

Status: Accepted
Date: 2026-08-06

ConnLens does not persist raw tokens. Secret-like values are hashed through `scan::secutil::fingerprint` and written as `sha256:xxxxxxxx`. The custom fixture grep script blocks raw fixture tokens outside `tests/fixtures/`.

This accepts that labels may be less human-friendly until probe enrichment arrives, but avoids turning ConnLens into a local secret cache.
