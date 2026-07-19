# AGENTS.md

## Project

Single-crate Rust CLI (`backup-system`) for backing up projects marked with `.backup` files to S3/R2. Containerized (Docker + `cargo-watch`). No CI, no linting config, no workspace.

## Key Commands

```bash
docker compose run --rm backup cargo run -- scan    # list discovered projects
docker compose run --rm backup cargo run -- audit   # size + skipped-file report
docker compose run --rm backup cargo run -- run      # backup + upload + retention + telegram
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
| `src/archive.rs` | ZIP creation from backup plans |
| `src/naming.rs` | Archive filename format + parsing |
| `src/storage.rs` | S3/R2 upload/download/delete/list |
| `src/telegram.rs` | Telegram notification sender |
| `src/tui.rs` | Terminal UI (ratatui) for browsing S3 backups |

## Backup Semantics

- `.backup` file marks a project root. Optional `#name` on first line overrides directory name.
- `.backupignore` uses `.gitignore` syntax, scoped to that project only.
- Nested `.backup` → separate child project/archive.
- Files over `MAX_FILE_SIZE_MB` or directories over `MAX_DIR_FILE_COUNT` are skipped + logged. Force-include with `!path` in `.backupignore`.
- Archive name format: `server__project__subproject--YYYY-MM-DD-HHMM-TZ.zip` — doubles as the hierarchy index.

## Config

All config is environment variables (loaded from `.env` via `dotenvy`). See `.env.example` for the full list. `.env` is gitignored.

## Gotchas

- Dockerfile uses `rust:1.85-bookworm` — keep Rust version aligned.
- `cargo-watch` is installed in the Docker image; it watches source for live rebuilds.
- The `BACKUP_SOURCE_DIR` env var is host-side only (used by `docker-compose.yml` volume mount), not read by the Rust code.
- S3 client uses `rustls-tls` (not native-tls). If adding crates that need TLS, prefer rustls-compatible options.
