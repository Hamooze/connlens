# ADR 0001: Tauri 2 + React + Rust

Status: Accepted
Date: 2026-08-06

ConnLens uses Tauri 2 with a Rust backend and Vite/React/TypeScript frontend. This matches the surge plan, keeps the tray utility small, and lets scanning and persistence live in a typed local backend instead of a browser-only app.

Alternatives considered: Electron and .NET desktop. Electron was rejected for footprint. .NET was not chosen because the product plan already standardizes on Rust/Tauri and agent-friendly local contracts.
