# AGENTS.md

This file gives coding agents (Claude Code, Codex, Cursor, Copilot, …) the context they need to work in this repo. End-user install guidance lives in [install.md](./install.md) — that is a different document for a different audience.

## What arx is

A self-hosted PaaS for home servers, written in Rust. Connect a GitHub repo; arx builds, routes through Traefik with ACME TLS, and keeps the service alive across zero-downtime swaps. Distributed as one daemon image (`ghcr.io/arxdevs/arx`) plus a single CLI binary (`arx`).

## Workspace layout

```
crates/
├─ arx-core/        domain types, ids, refs parser, config, errors
├─ arx-db/          sqlx-sqlite + ChaCha20-Poly1305 variable crypto
├─ arx-docker/      bollard wrapper (ContainerEngine trait)
├─ arx-traefik/     dynamic.yml renderer + atomic writer
├─ arx-build/       stack detect + Dockerfile templates + docker build
├─ arx-github/      webhook HMAC verify + manifest helpers
├─ arx-server/      axum HTTP API, deploy pipeline, schedulers
└─ arx-cli/         clap CLI (the `arx` binary)
migrations/         sqlx SQL files (runtime-applied)
compose.yml         two-container daemon stack (arx + traefik)
```

Dependencies: `core → db → server`, `core → build → server`, `docker / traefik / github → server`. CLI talks to server over HTTP only.

## Build, test, lint

Always run from the workspace root.

```bash
cargo check --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
cargo fmt --all -- --check
```

`--locked` is mandatory in CI; use it locally too so you don't drift from `Cargo.lock`.

Build the daemon image locally with `docker build -t arx:local -f Dockerfile .`. The CLI binary builds with `cargo build --release --bin arx`.

## Conventions

- **Edition 2024**, MSRV 1.85 (set in workspace).
- **Error handling**: `arx_core::Error` for cross-crate errors; map at API boundary via `ApiError`. Never `unwrap()` or `expect()` in production code paths.
- **No new `unsafe`** unless the unsafe block has a comment explaining why and what invariant it upholds.
- **Logging**: `tracing` macros. No `println!` in library or server code.
- **Async**: tokio runtime, `async fn`. Spawning long-running tasks goes through `state::AppState` so they share lifetimes.
- **DB queries**: live in `arx-db/src/queries/<noun>.rs`. Always parameterise (`sqlx::query(...).bind(...)`). Never `format!`-build SQL.
- **First-class service settings**: build/start commands and similar knobs are columns on `services`, never env-var piggybacks. Adding a new knob = migration + model field + queries + api handler + cli flag.
- **Stack templates**: any new stack adds one file in `crates/arx-build/src/stacks/`, plus a registration line in `crates/arx-build/src/stack.rs::detect_stack`.
- **Input validation**: every value that ends up inside a generated Dockerfile, a `docker` argv, or a `git` argv must pass through `crates/arx-build/src/validate.rs` first. Bypassing it is an injection bug.

## Commit conventions

- One purpose per commit. Err on the side of more commits, not fewer.
- Format: `<type>: <description>`.
  - Types: `feat`, `fix`, `chore`, `refactor`, `docs`, `test`, `perf`.
  - English, lowercase, single line, no period at the end, no scope.
- No `Co-Authored-By` or assistant attribution trailers.

## Release process

- Releases are `vX.Y.Z` git tags on `main`. The `release.yml` workflow handles the rest (cross-platform binaries, GitHub Release, ghcr.io image).
- For version bumps after v0.1.0, prefer `cargo release patch|minor|major --execute` (configured via `release.toml`).
- CHANGELOG.md is intentionally not maintained — release notes live on each GitHub Release, auto-generated from PR titles. Use PR-only flow on `main` so auto-notes have content.

## Things not to do

- **Do not** reintroduce a runtime build agent (railpack, nixpacks, buildkit container, etc.). The decision is to keep stack templates inline; see commit history for the migration that removed buildkit.
- **Do not** expose port 7878 publicly. The daemon binds `127.0.0.1:7878`; Traefik routes `arx.<root-domain>` to it.
- **Do not** write generated Dockerfiles into the user's repo. Use the existing `docker_build_stdin` path (`docker build -f -`).
- **Do not** commit `compose.override.yml`. It is gitignored — it exists only as a local dev override for `arx:local`.
- **Do not** commit `master.key`, `.env`, SQLite databases, or anything from `/tmp/arx-test/`.
- **Do not** weaken the security boundaries documented in README §"Security model". Path traversal, command injection, and SQL injection guards exist deliberately.
- **Do not** push directly to `main` for non-trivial changes. Open a PR so the auto-generated release notes have something to draw from.
- **Do not** force-push `main` or any tag.

## Repository facts

- License: dual MIT / Apache-2.0 (see `LICENSE-MIT`, `LICENSE-APACHE`).
- Remote: `https://github.com/arxdevs/arx`. Default branch `main`.
- CI is in `.github/workflows/ci.yml` (push + PR); release is `.github/workflows/release.yml` (tag).
