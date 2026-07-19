"""Archive filename *is* the index: servername__project__subproject_DATE+TZ.zip"""
import re
from datetime import datetime
from zoneinfo import ZoneInfo  # stdlib (py3.9+), no need for pytz

DATE_RE = re.compile(r"^(?P<leaf>.+)_(?P<date>\d{4}-\d{2}-\d{2}-\d{4}[+-]\d{4})$")


def build_name(server: str, project_chain: list[str], tz_name: str, when: datetime | None = None) -> str:
    tz = ZoneInfo(tz_name)
    when = (when or datetime.now(tz)).astimezone(tz)
    date_str = when.strftime("%Y-%m-%d-%H%M%z")
    chain = "__".join(project_chain)
    return f"{server}__{chain}_{date_str}.zip"


def parse_name(filename: str) -> dict | None:
    if not filename.endswith(".zip"):
        return None
    parts = filename[: -len(".zip")].split("__")
    if len(parts) < 2:
        return None
    server, *chain = parts
    m = DATE_RE.match(chain[-1])
    if not m:
        return None
    chain[-1] = m.group("leaf")
    return {"server": server, "chain": chain, "date": m.group("date")}
