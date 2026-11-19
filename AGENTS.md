# AGENTS.md

## Project

Single-crate Rust CLI (`backup-system`) for backing up projects marked with `.backup` files to S3/R2. Release builds come from GitHub Actions (`.github/workflows/release.yml`): every push to `master` publishes
`ghcr.io/n1ades/backup-system:latest` (+ `sha-<commit>`), tags `v*` also attach the binary to a GitHub Release.
The server only pulls that slim image (debian-slim + binary); it never compiles. No linting config, no workspace.

## Key Commands

```bash
docker compose run --rm backup scan    # list discovered projects
docker compose run --rm backup audit   # size + skipped-file report
docker compose run --rm backup tree     # preview the snapshot tree (included + skipped)
docker compose run --rm backup run      # one snapshot zip + upload + retention + telegram
docker compose run --rm backup run --dry-run  # build the zip into DOWNLOAD_DIR, no upload
docker compose run --rm backup tui      # browse/download/extract past backups
producer | docker compose run --rm -T backup put NAME --ext dump  # stream stdin to one object
docker compose run --rm -T backup get KEY > file      # stream one object to stdout

# Deploy: push to master, wait for the release workflow, then on the server
docker compose pull

# Dev: anywhere with Rust 1.85
cargo test
cargo build --release
```

On the server always run via `docker compose` — it supplies `.env` and mounts the host `/srv` read-only.

## Architecture

| File | Role |
|---|---|
| `src/main.rs` | CLI entrypoint (clap), orchestration |
| `src/config.rs` | Env-based config via `dotenvy` |
| `src/project.rs` | `.backup` marker discovery, ignore rules, backup planning |
| `src/snapshot.rs` | Merges all project plans into one server snapshot; renders the annotated tree |
| `src/archive.rs` | ZIP creation (files + in-memory extras such as the tree) |
| `src/naming.rs` | Object name format (`server__chain--stamp-TZ.ext`) + parsing, timestamps |
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
- `put NAME --ext EXT` streams stdin into `server__NAME--YYYY-MM-DD-HHMM-TZ.EXT` (no local file) and applies the same retention. Callers: `/srv/vrchat-catalog/srv/deploy/backup.py` (DropCat dumps). Callers use `docker compose run --rm -T backup put ...`.
- Retention keeps `RETENTION_KEEP` objects per name chain; `RETENTION_KEEP_OVERRIDES=chain=n,...` sets it per chain (DropCat dumps keep 3).
- Old per-project archives (`server__project--...zip`) may still be in the bucket; retention groups by name chain and never deletes them once no new ones appear.

## Config

All config is environment variables (loaded from `.env` via `dotenvy`). See `.env.example` for the full list. `.env` is gitignored.

## Gotchas

- Dockerfile build stage and the workflow use Rust 1.85 — keep them aligned.
- Never build on the server: compiling under the 256M compose limit OOM-kills the linker.
- `.dockerignore` keeps `.env` out of the build context: the image is public.
- The `BACKUP_SOURCE_DIR` env var is host-side only (used by `docker-compose.yml` volume mount), not read by the Rust code.
- `rust-s3` is built with `default-features = false`, so `fail-on-err` must stay in the feature list in `Cargo.toml`; without it S3 errors (403, etc.) pass silently and runs print success.
- Uploads go through `storage::upload_stream`: sequential multipart, one 8 MiB part in memory, size checked by HEAD afterwards. Do not use `Bucket::put_object_stream`: it queues every part before sending, so memory grows with the object and a 1 GB dump exceeds the 256M cap. Downloads stream too (`get_object_to_writer`).
- Building (linking) needs more than the 256M cap; build once with a temporary compose override of `deploy.resources.limits.memory` (~1.5G). Running the built binary fits in 256M.
- S3 client uses `rustls-tls` (not native-tls). If adding crates that need TLS, prefer rustls-compatible options.
