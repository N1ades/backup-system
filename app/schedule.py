"""Just cron-expression validation. Scheduling itself now lives per-project
in each .backup file (see app/settings.py) and is re-read on every run, so
there's no separate "current schedule" state to store or push live anymore."""


def validate(expr: str) -> bool:
    # ponytail: structural check only (5 whitespace-separated fields), not
    # per-field range/step validation -- croniter raises a clear error at
    # match-time for anything genuinely malformed, which is enough here.
    return len(expr.split()) == 5
