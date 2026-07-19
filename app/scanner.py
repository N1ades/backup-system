"""Walk /srv, find .backup roots (nested ones become sub-projects), and
classify every file under a project as included / excluded / unmarked."""
from __future__ import annotations
from dataclasses import dataclass, field
from pathlib import Path

from app.ignore import IGNORE_FILENAME, MARKER_FILENAME, build_ignore_spec, parse_backup_file
from app.config import Config
from app.settings import ProjectSettings, resolve as resolve_settings


@dataclass
class Project:
    path: Path
    name: str
    settings: ProjectSettings
    include_spec: object            # pathspec.PathSpec
    parent: "Project | None" = None
    children: list["Project"] = field(default_factory=list)
    settings_warnings: list[str] = field(default_factory=list)

    def chain(self) -> list[str]:
        return (self.parent.chain() if self.parent else []) + [self.name]


@dataclass
class FileVerdict:
    path: Path          # absolute
    included: bool
    reason: str          # "backupignore" | "backup-pattern" | "unmarked" | "unmarked-too-big"
    size: int = 0


@dataclass
class ScanResult:
    project: Project
    included: list[FileVerdict] = field(default_factory=list)
    excluded: list[FileVerdict] = field(default_factory=list)
    unmarked_files: list[FileVerdict] = field(default_factory=list)   # included but not explicitly marked either way
    too_big_dirs: list[tuple[Path, int, int]] = field(default_factory=list)  # path, size, filecount

    @property
    def total_size(self) -> int:
        return sum(f.size for f in self.included)


def build_project_tree(base: Path, cfg: Config) -> list[Project]:
    """Top-level projects directly under base; nested .backup dirs become
    children (own archive, but shown as part of the hierarchy)."""
    roots: list[Project] = []

    def walk(dir_: Path, parent: "Project | None"):
        try:
            entries = sorted(p for p in dir_.iterdir() if p.is_dir())
        except OSError:
            return
        for entry in entries:
            marker = entry / MARKER_FILENAME
            if marker.exists():
                directives, include_spec = parse_backup_file(marker)
                settings, warnings = resolve_settings(directives, cfg)
                proj = Project(
                    path=entry,
                    name=settings.name or entry.name,
                    settings=settings,
                    include_spec=include_spec,
                    parent=parent,
                    settings_warnings=warnings,
                )
                (parent.children if parent else roots).append(proj)
                walk(entry, proj)  # look for nested projects inside this one
            else:
                walk(entry, parent)  # keep searching deeper, same project context

    walk(base, None)
    return roots


def flatten(projects: list[Project]) -> list[Project]:
    out = []
    for p in projects:
        out.append(p)
        out.extend(flatten(p.children))
    return out


def _dir_stats(path: Path, stop_dirs: set[Path]) -> tuple[int, int]:
    """Total size (bytes) and file count under path, not descending into stop_dirs."""
    total_size = 0
    total_files = 0
    for f in path.rglob("*"):
        if f.is_dir():
            continue
        if any(_is_within(f, s) for s in stop_dirs):
            continue
        try:
            total_size += f.stat().st_size
            total_files += 1
        except OSError:
            pass
    return total_size, total_files


def _is_within(path: Path, ancestor: Path) -> bool:
    try:
        path.relative_to(ancestor)
        return True
    except ValueError:
        return False


def scan_project(project: Project, cfg: Config) -> ScanResult:
    stop_dirs = {c.path for c in project.children}
    ignore_spec = build_ignore_spec(project.path, stop_dirs)
    backup_spec = project.include_spec

    result = ScanResult(project=project)
    max_bytes = cfg.audit_max_dir_mb * 1024 * 1024

    def walk(dir_: Path):
        try:
            entries = sorted(dir_.iterdir())
        except OSError:
            return
        for entry in entries:
            if entry in stop_dirs:
                continue  # nested project, backed up separately
            rel = entry.relative_to(project.path)
            rel_str = str(rel) + ("/" if entry.is_dir() else "")

            if ignore_spec.match_file(rel_str):
                if entry.is_file():
                    result.excluded.append(FileVerdict(entry, False, "backupignore", entry.stat().st_size))
                continue  # strict: never descend into an ignored dir either

            explicitly_included = backup_spec.match_file(rel_str)

            if entry.is_dir():
                if explicitly_included:
                    walk(entry)  # forced include: size limits don't apply
                    continue
                size, count = _dir_stats(entry, stop_dirs)
                if size > max_bytes or count > cfg.audit_max_dir_files:
                    result.too_big_dirs.append((entry, size, count))
                    continue  # excluded, unmarked, too big -> skip whole subtree
                walk(entry)
                continue

            # file
            size = entry.stat().st_size
            if explicitly_included:
                fv = FileVerdict(entry, True, "backup-pattern", size)
                result.included.append(fv)
            else:
                fv = FileVerdict(entry, True, "unmarked", size)
                result.included.append(fv)
                result.unmarked_files.append(fv)

    walk(project.path)
    return result
