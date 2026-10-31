# AGENTS.md

## Project

Single-crate Rust CLI (`backup-system`) for backing up projects marked with `.backup` files to S3/R2. Containerized (Docker + `cargo-watch`). No CI, no linting config, no workspace.

## Key Commands

```bash
docker compose run --rm backup cargo run -- scan    # list discovered projects
docker compose run --rm backup cargo run -- audit   # size + skipped-file report
docker compose run --rm backup cargo run -- tree     # preview the snapshot tree (included + skipped)
docker compose run --rm backup cargo run -- run      # one snapshot zip + upload + retention + telegram
docker compose run --rm backup cargo run -- run --dry-run  # build the zip into DOWNLOAD_DIR, no upload
docker compose run --rm backup cargo run -- tui      # browse/download/extract past backups

# Dev: rebuild inside container (source is bind-mounted)
docker compose run --rm backup cargo build
docker compose run --rm backup cargo test
```

There is no standalone `cargo run` workflow — the container mounts `.env` and the host `/srv` directory. Always run via `docker compose`.

## Architecture

| File | Role |
|---|---|
| `src/main.rs` | CLI entrypoint (clap), orchestration |
| `src/config.rs` | Env-based config via `dotenvy` |
| `src/project.rs` | `.backup` marker discovery, ignore rules, backup planning |
| `src/snapshot.rs` | Merges all project plans into one server snapshot; renders the annotated tree |
| `src/archive.rs` | ZIP creation (files + in-memory extras such as the tree) |
| `src/naming.rs` | Archive filename format + parsing |
| `src/storage.rs` | S3/R2 upload/download/delete/list |
| `src/telegram.rs` | Telegram notification sender |
| `src/tui.rs` | Terminal UI (ratatui) for browsing S3 backups |

## Backup Semantics

- `.backup` file marks a project root. Optional `#name` on first line overrides directory name.
- `.backupignore` uses `.gitignore` syntax, scoped to that project only.
- Nested `.backup` → separate child project, but all projects go into the same snapshot archive.
- Files over `MAX_FILE_SIZE_MB` or directories over `MAX_DIR_FILE_COUNT` are skipped + logged. Force-include with `!path` in `.backupignore`.
- One snapshot per run: `server--YYYY-MM-DD-HHMM-TZ.zip`. Paths inside are relative to `BACKUP_ROOT`, so one extract restores the tree.
- The first zip entry `_snapshot/TREE.txt` is an annotated directory tree: every included file with its size, and every `[SKIPPED: reason]` entry (ignored, oversized, non-regular, or outside any project).
- Old per-project archives (`server__project--...zip`) may still be in the bucket; retention groups by name chain and never deletes them once no new ones appear.

## Config

All config is environment variables (loaded from `.env` via `dotenvy`). See `.env.example` for the full list. `.env` is gitignored.

## Gotchas

- Dockerfile uses `rust:1.85-bookworm` — keep Rust version aligned.
- `cargo-watch` is installed in the Docker image; it watches source for live rebuilds.
- The `BACKUP_SOURCE_DIR` env var is host-side only (used by `docker-compose.yml` volume mount), not read by the Rust code.
- `rust-s3` is built with `default-features = false`, so `fail-on-err` must stay in the feature list in `Cargo.toml`; without it S3 errors (403, etc.) pass silently and runs print success.
- Uploads stream via multipart (`put_object_stream`); the compose memory cap is 256M and a full in-memory read of the snapshot would exceed it.
- Building (linking) needs more than the 256M cap; build once with a temporary compose override of `deploy.resources.limits.memory` (~1.5G). Running the built binary fits in 256M.
- S3 client uses `rustls-tls` (not native-tls). If adding crates that need TLS, prefer rustls-compatible options.
