from collections import defaultdict
import tempfile
import shutil
from datetime import datetime, timedelta, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

from croniter import croniter

from app.config import Config, load_config
from app.scanner import build_project_tree, flatten, scan_project
from app.archiver import create_archive
from app.naming import build_name, parse_name
from app.storage import upload, list_objects, delete, bucket_usage_bytes
from app.telegram_notify import send
from app.audit import audit_project
from app.sizes import human


def is_due(cron_expr: str, now: datetime) -> bool:
    return croniter.match(cron_expr, now)


def run_backup(cfg: Config | None = None, only_due: bool = False) -> list[str]:
    """only_due=True is what the every-minute cron trigger uses: each
    project carries its own schedule (from .backup, default cfg.backup_cron),
    so this checks each one independently instead of running them all at
    once. "Run backup now" from the CLI passes only_due=False to back up
    everything regardless of schedule."""
    cfg = cfg or load_config()
    projects = flatten(build_project_tree(cfg.backup_root, cfg))
    now = datetime.now(ZoneInfo(cfg.timezone))
    lines = []
    total_size = 0
    ran_any = False

    with tempfile.TemporaryDirectory() as tmp:
        for project in projects:
            if only_due and not is_due(project.settings.cron, now):
                continue
            ran_any = True
            result = scan_project(project, cfg)
            report = audit_project(result, cfg)
            name = build_name(cfg.server_name, project.chain(), cfg.timezone, now)
            local_zip = Path(tmp) / name
            create_archive(result, local_zip)
            upload(cfg, local_zip, name)
            total_size += report.included_size
            warn = f" \u26A0\uFE0F {report.unmarked_count} unmarked" if report.unmarked_count else ""
            lines.append(f"  {'/'.join(project.chain())}: {human(report.included_size)}{warn}")

    if not ran_any:
        return lines  # nothing due this minute -- stay quiet, no telegram spam

    lines.insert(0, f"\U0001F4E6 Backup on {cfg.server_name}")
    removed = run_retention(cfg, projects)
    if removed:
        lines.append(f"\U0001F5D1 Removed {len(removed)} expired/over-limit archive(s)")

    used = shutil.disk_usage(cfg.backup_root)
    lines.append(f"\U0001F4BE Local disk free: {human(used.free)} / {human(used.total)}")
    try:
        lines.append(f"\u2601\uFE0F Storage used: {human(bucket_usage_bytes(cfg))}")
    except Exception:
        pass
    lines.append(f"\u2705 Done. Backed up this run: {human(total_size)}")

    send(cfg, "\n".join(lines))
    return lines


def run_retention(cfg: Config, projects: list) -> list[str]:
    """Per project: keep_versions newest, drop anything older than
    keep_days, then trim oldest-first until under max_total_size_bytes.
    The single most recent archive is never deleted, even if a project's
    own settings are misconfigured -- retention must never delete the only
    copy of a backup."""
    objects = list_objects(cfg, prefix=f"{cfg.server_name}__")
    grouped: dict[tuple, list[tuple[str, int, datetime]]] = defaultdict(list)
    for obj in objects:
        parsed = parse_name(obj["Key"])
        if not parsed or parsed["server"] != cfg.server_name:
            continue
        when = datetime.strptime(parsed["date"], "%Y-%m-%d-%H%M%z")
        grouped[tuple(parsed["chain"])].append((obj["Key"], obj["Size"], when))

    settings_by_chain = {tuple(p.chain()): p.settings for p in projects}
    now = datetime.now(timezone.utc)
    removed: list[str] = []

    for chain, versions in grouped.items():
        versions.sort(key=lambda v: v[2], reverse=True)  # newest first
        newest, rest = versions[0], versions[1:]
        settings = settings_by_chain.get(chain)
        keep_days = settings.keep_days if settings else cfg.retention_days
        keep_versions = settings.keep_versions if settings else None
        max_bytes = settings.max_total_size_bytes if settings else None

        keep = [newest]
        drop = []
        for i, (key, size, when) in enumerate(rest, start=1):
            age_ok = (now - when) <= timedelta(days=keep_days)
            count_ok = keep_versions is None or i < keep_versions
            (keep if (age_ok and count_ok) else drop).append((key, size, when))

        if max_bytes is not None:
            keep.sort(key=lambda v: v[2])  # oldest first, so we trim from the back
            running = sum(s for _, s, _ in keep)
            while running > max_bytes and len(keep) > 1:
                key, size, when = keep.pop(0)
                drop.append((key, size, when))
                running -= size

        for key, _, _ in drop:
            delete(cfg, key)
            removed.append(key)

    return removed
