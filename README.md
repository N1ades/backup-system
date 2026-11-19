# backup-system

Rust CLI for tree-based project backups to S3/R2.

## Setup

```
cp .env.example .env   # fill in S3/R2 + server name
docker compose build
```

## Use

```
docker compose run --rm backup scan    # list found .backup projects
docker compose run --rm backup audit   # size + skipped-file report
docker compose run --rm backup tree     # preview the snapshot tree (included + skipped)
docker compose run --rm backup run      # one snapshot + upload + retention + telegram
docker compose run --rm backup tui      # browse/download/extract past backups
producer | docker compose run --rm -T backup put NAME --ext dump
docker compose run --rm -T backup get KEY > file
```

The image `ghcr.io/n1ades/backup-system` is built by GitHub Actions on every push to `master`
(tags `v*` also publish a GitHub Release with the binary). Update the server with `docker compose pull`.

## Rules recap

- `.backup` marks a project root (optional `#name` first line).
- `.backupignore` = strict `.gitignore` syntax, scoped to that project only.
- Nested `.backup` → separate child project, stored in the same snapshot.
- Files/dirs over `MAX_FILE_SIZE_MB` / `MAX_DIR_FILE_COUNT` are skipped + logged
  in audit, unless force-included with `!path` in `.backupignore`.
- One snapshot per run: `server--YYYY-MM-DD-HHMM-TZ.zip`, containing
  `_snapshot/TREE.txt` (everything included, and everything skipped with the reason).
