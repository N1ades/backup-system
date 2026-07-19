"""Config: just env vars, no framework needed for this many fields."""
import os
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class Config:
    server_name: str = os.environ.get("SERVER_NAME", "server")
    timezone: str = os.environ.get("TIMEZONE", "UTC")
    backup_root: Path = Path(os.environ.get("BACKUP_ROOT", "/srv"))
    restore_root: Path = Path(os.environ.get("RESTORE_ROOT", "/restore"))

    s3_endpoint_url: str = os.environ.get("S3_ENDPOINT_URL", "")
    s3_bucket: str = os.environ.get("S3_BUCKET", "")
    s3_access_key: str = os.environ.get("S3_ACCESS_KEY_ID", "")
    s3_secret_key: str = os.environ.get("S3_SECRET_ACCESS_KEY", "")
    s3_region: str = os.environ.get("S3_REGION", "auto")

    # audit / "unmarked but huge" thresholds (rung: everything configurable, nothing hardcoded)
    audit_max_dir_mb: float = float(os.environ.get("AUDIT_MAX_DIR_MB", "10"))
    audit_max_dir_files: int = int(os.environ.get("AUDIT_MAX_DIR_FILES", "1000"))
    audit_warn_file_mb: float = float(os.environ.get("AUDIT_WARN_FILE_MB", "50"))

    retention_days: int = int(os.environ.get("RETENTION_DAYS", "30"))

    telegram_token: str = os.environ.get("TELEGRAM_BOT_TOKEN", "")
    telegram_chat_id: str = os.environ.get("TELEGRAM_CHAT_ID", "")

    backup_cron: str = os.environ.get("BACKUP_CRON", "0 3 * * *")


def load_config() -> Config:
    # load .env if present (no python-dotenv needed for a single optional file)
    env_file = Path(os.environ.get("ENV_FILE", ".env"))
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, v = line.split("=", 1)
            os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))
    return Config()
