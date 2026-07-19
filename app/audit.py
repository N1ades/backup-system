from dataclasses import dataclass
from app.scanner import ScanResult, Project, flatten, scan_project
from app.config import Config


@dataclass
class AuditReport:
    project: Project
    included_size: int
    unmarked_count: int
    unmarked_size: int
    too_big_dirs: list[tuple]  # (path, size, filecount)
    large_files: list[tuple]   # (path, size) among included, over threshold


def audit_project(result: ScanResult, cfg: Config) -> AuditReport:
    warn_bytes = cfg.audit_warn_file_mb * 1024 * 1024
    large_files = [(f.path, f.size) for f in result.included if f.size > warn_bytes]
    return AuditReport(
        project=result.project,
        included_size=result.total_size,
        unmarked_count=len(result.unmarked_files),
        unmarked_size=sum(f.size for f in result.unmarked_files),
        too_big_dirs=result.too_big_dirs,
        large_files=large_files,
    )


def audit_all(projects: list[Project], cfg: Config) -> list[AuditReport]:
    return [audit_project(scan_project(p, cfg), cfg) for p in flatten(projects)]
