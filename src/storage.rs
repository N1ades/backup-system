use crate::config::Config;
use anyhow::{Context, Result};
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use std::path::Path;

pub struct RemoteObject {
    pub key: String,
    pub size: u64,
}

pub fn bucket(cfg: &Config) -> Result<Box<Bucket>> {
    let region = Region::Custom {
        region: cfg.s3_region.clone(),
        endpoint: cfg.s3_endpoint.clone(),
    };
    let credentials = Credentials::new(
        Some(&cfg.s3_access_key),
        Some(&cfg.s3_secret_key),
        None,
        None,
        None,
    )?;
    let mut b = Bucket::new(&cfg.s3_bucket, region, credentials)?;
    if cfg.s3_path_style {
        b = b.with_path_style();
    }
    Ok(b)
}

fn s3_key(key: &str) -> &str {
    key.strip_prefix('/').unwrap_or(key)
}

// Streamed (multipart) so a snapshot of any size never has to fit in memory.
pub async fn upload_file(bucket: &Bucket, local_path: &Path, key: &str) -> Result<()> {
    let mut file = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to open file for upload: {e}"))?;
    let status = bucket
        .put_object_stream(&mut file, s3_key(key))
        .await
        .with_context(|| format!("S3 upload of {key} failed"))?;
    let status = status.status_code();
    if !(200..300).contains(&status) {
        anyhow::bail!("S3 upload of {key} returned status {status}");
    }
    Ok(())
}

pub async fn download_file(bucket: &Bucket, key: &str, dest: &Path) -> Result<()> {
    let response = bucket.get_object(s3_key(key)).await?;
    let status = response.status_code();
    if status != 200 {
        anyhow::bail!("S3 GET {key} returned status {status}");
    }
    let bytes = response.bytes().to_vec();
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::write(dest, bytes))
        .await?
        .map_err(|e| anyhow::anyhow!("failed to write downloaded file: {e}"))?;
    Ok(())
}

pub async fn delete_object(bucket: &Bucket, key: &str) -> Result<()> {
    bucket
        .delete_object(s3_key(key))
        .await
        .with_context(|| format!("S3 delete of {key} failed"))?;
    Ok(())
}

pub async fn list_all(bucket: &Bucket) -> Result<Vec<RemoteObject>> {
    let pages = bucket
        .list("".to_string(), None)
        .await
        .context("S3 bucket listing failed")?;
    let mut out = Vec::new();
    for page in pages {
        for obj in page.contents {
            out.push(RemoteObject {
                key: obj.key,
                size: obj.size,
            });
        }
    }
    Ok(out)
}

pub async fn used_bytes(bucket: &Bucket) -> Result<u64> {
    let objects = list_all(bucket).await?;
    Ok(objects.iter().map(|o| o.size).sum())
}
