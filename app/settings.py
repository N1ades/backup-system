"""Turn the raw directives parsed out of a .backup file into typed,
validated settings -- falling back to global Config where a project
doesn't specify its own, and never raising (a bad directive becomes a
warning + a safe default, not a crashed backup run)."""
from dataclasses import dataclass

from app.config import Config
from app import sizes
from app import schedule


@dataclass
class ProjectSettings:
    name: str | None
    cron: str                          # always valid, falls back to cfg.backup_cron
    keep_versions: int | None          # None = no count limit
    keep_days: int                     # falls back to cfg.retention_days
    max_total_size_bytes: int | None   # None = no size limit


def resolve(directives: dict[str, str], cfg: Config) -> tuple[ProjectSettings, list[str]]:
    warnings: list[str] = []

    cron = directives.get("cron", "").strip() or cfg.backup_cron
    if not schedule.validate(cron):
        warnings.append(f"invalid cron '{cron}' in .backup, using default '{cfg.backup_cron}'")
        cron = cfg.backup_cron

    keep_versions = None
    if "keep_versions" in directives:
        try:
            keep_versions = int(directives["keep_versions"])
        except ValueError:
            warnings.append(f"invalid keep_versions '{directives['keep_versions']}', ignoring")

    keep_days = cfg.retention_days
    if "keep_days" in directives:
        try:
            keep_days = int(directives["keep_days"])
        except ValueError:
            warnings.append(f"invalid keep_days '{directives['keep_days']}', using default {cfg.retention_days}")

    max_total_size_bytes = None
    if "max_total_size" in directives:
        try:
            max_total_size_bytes = sizes.parse(directives["max_total_size"])
        except ValueError as e:
            warnings.append(str(e))

    return ProjectSettings(
        name=directives.get("name"),
        cron=cron,
        keep_versions=keep_versions,
        keep_days=keep_days,
        max_total_size_bytes=max_total_size_bytes,
    ), warnings
