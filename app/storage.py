from pathlib import Path
import boto3
from app.config import Config


def client(cfg: Config):
    return boto3.client(
        "s3",
        endpoint_url=cfg.s3_endpoint_url or None,
        aws_access_key_id=cfg.s3_access_key,
        aws_secret_access_key=cfg.s3_secret_key,
        region_name=cfg.s3_region,
    )


def upload(cfg: Config, local_path: Path, key: str):
    client(cfg).upload_file(str(local_path), cfg.s3_bucket, key)


def download(cfg: Config, key: str, local_path: Path):
    local_path.parent.mkdir(parents=True, exist_ok=True)
    client(cfg).download_file(cfg.s3_bucket, key, str(local_path))


def delete(cfg: Config, key: str):
    client(cfg).delete_object(Bucket=cfg.s3_bucket, Key=key)


def list_objects(cfg: Config, prefix: str = "") -> list[dict]:
    c = client(cfg)
    out = []
    paginator = c.get_paginator("list_objects_v2")
    for page in paginator.paginate(Bucket=cfg.s3_bucket, Prefix=prefix):
        out.extend(page.get("Contents", []))
    return out


def bucket_usage_bytes(cfg: Config) -> int:
    return sum(o["Size"] for o in list_objects(cfg))
