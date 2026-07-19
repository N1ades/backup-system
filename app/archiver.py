import zipfile
from pathlib import Path
from app.scanner import ScanResult


def create_archive(result: ScanResult, dest_zip: Path) -> Path:
    dest_zip.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(dest_zip, "w", zipfile.ZIP_DEFLATED) as zf:
        for fv in result.included:
            arcname = fv.path.relative_to(result.project.path)
            zf.write(fv.path, arcname)
    return dest_zip
