import zipfile
import tempfile
from pathlib import Path

from app.config import Config
from app.naming import parse_name
from app.storage import download


def restore(cfg: Config, key: str, dest_root: Path | None = None) -> Path:
    """Downloads `key` and unzips it under dest_root/<chain>/ so restored
    projects land back in the same nested layout they were backed up from."""
    parsed = parse_name(key)
    if not parsed:
        raise ValueError(f"'{key}' doesn't look like a backup archive name")
    dest_root = dest_root or cfg.restore_root
    target = dest_root.joinpath(*parsed["chain"])
    with tempfile.TemporaryDirectory() as tmp:
        local_zip = Path(tmp) / key
        download(cfg, key, local_zip)
        target.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(local_zip) as zf:
            zf.extractall(target)
    return target
