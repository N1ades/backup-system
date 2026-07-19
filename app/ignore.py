"""
.backupignore / .backup pattern matching, built on `pathspec` (the same
gitwildmatch engine git itself uses) instead of hand-rolling glob logic.

ponytail: cumulative-exclude model. Patterns from every .backupignore file
inside a project are unioned (any file matching ANY of them is excluded).
`!negation` works correctly *within* a single .backupignore file (pathspec
handles that), but a deeper .backupignore cannot currently "un-ignore" a
path excluded by a shallower one in the same project -- true git semantics
scope each file to its own directory and let deeper files win. If that's
ever needed: build one PathSpec per directory and evaluate deepest-first,
using the first directory whose patterns actually reference the path.
"""
from pathlib import Path
import pathspec

IGNORE_FILENAME = ".backupignore"
MARKER_FILENAME = ".backup"


def _read_lines(path: Path) -> list[str]:
    try:
        return path.read_text(encoding="utf-8", errors="ignore").splitlines()
    except OSError:
        return []


def build_ignore_spec(project_root: Path, stop_dirs: set[Path]) -> pathspec.PathSpec:
    """Collect every .backupignore under project_root (not crossing into
    nested project dirs in `stop_dirs`) into one cumulative PathSpec.
    Patterns are rewritten relative to project_root so e.g. `node_modules`
    inside a subdir still only matches under that subdir, matching normal
    gitignore-in-a-subfolder behavior for anchored/relative patterns.
    """
    lines: list[str] = []
    for ignore_file in sorted(project_root.rglob(IGNORE_FILENAME)):
        rel_dir = ignore_file.parent.relative_to(project_root)
        if any(_is_within(ignore_file.parent, stop) for stop in stop_dirs):
            continue
        prefix = "" if rel_dir == Path(".") else str(rel_dir) + "/"
        for raw in _read_lines(ignore_file):
            stripped = raw.strip()
            if not stripped or stripped.startswith("#"):
                continue
            neg = stripped.startswith("!")
            body = stripped[1:] if neg else stripped
            body = body.lstrip("/")
            lines.append(("!" if neg else "") + prefix + body)
    return pathspec.PathSpec.from_lines("gitwildmatch", lines)


DIRECTIVE_KEYS = {"name", "cron", "keep_versions", "keep_days", "max_total_size"}


def parse_backup_file(marker_file: Path) -> tuple[dict[str, str], pathspec.PathSpec]:
    """.backup format:
        #projectname                  <- legacy shorthand for #name: projectname (first line only)
        #cron: 0 */6 * * *
        #keep_versions: 10
        #keep_days: 30
        #max_total_size: 5GB
        some/include/pattern           <- gitignore-syntax, weaker than .backupignore

    Any other '#' line is a plain comment (ignored), same as .gitignore.
    Returns (directives, include_pathspec).
    """
    directives: dict[str, str] = {}
    pattern_lines: list[str] = []
    lines = _read_lines(marker_file)

    if lines and lines[0].strip().startswith("#") and ":" not in lines[0]:
        legacy_name = lines[0].strip().lstrip("#").strip()
        if legacy_name:
            directives["name"] = legacy_name
        lines = lines[1:]

    for raw in lines:
        stripped = raw.strip()
        if not stripped:
            continue
        if stripped.startswith("#"):
            body = stripped[1:].strip()
            if ":" in body:
                key, _, value = body.partition(":")
                key = key.strip().lower()
                if key in DIRECTIVE_KEYS:
                    directives[key] = value.strip()
            continue
        pattern_lines.append(stripped)

    return directives, pathspec.PathSpec.from_lines("gitwildmatch", pattern_lines)


def _is_within(path: Path, ancestor: Path) -> bool:
    try:
        path.relative_to(ancestor)
        return True
    except ValueError:
        return False
