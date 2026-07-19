import re

_UNITS = {"B": 1, "KB": 1024, "MB": 1024**2, "GB": 1024**3, "TB": 1024**4, "PB": 1024**5}
_SIZE_RE = re.compile(r"^\s*([\d.]+)\s*([KMGTP]?B)\s*$", re.IGNORECASE)


def human(n: float) -> str:
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if n < 1024:
            return f"{n:.1f}{unit}"
        n /= 1024
    return f"{n:.1f}PB"


def parse(s: str) -> int:
    """'5GB' / '500 MB' -> bytes. Raises ValueError on anything else."""
    m = _SIZE_RE.match(s)
    if not m:
        raise ValueError(f"'{s}' is not a size like '500MB' or '5GB'")
    value, unit = m.groups()
    return int(float(value) * _UNITS[unit.upper()])
