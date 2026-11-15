use crate::config::Config;
use anyhow::{Context, Result};
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

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

const PART_SIZE: usize = s3::bucket::CHUNK_SIZE;
const CONTENT_TYPE: &str = "application/octet-stream";

async fn read_part<R: AsyncRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>) -> Result<()> {
    buf.clear();
    while buf.len() < PART_SIZE {
        let n = (&mut *reader)
            .take((PART_SIZE - buf.len()) as u64)
            .read_to_end(buf)
            .await?;
        if n == 0 {
            break;
        }
    }
    Ok(())
}

/// Multipart upload that holds one part in memory at a time.
/// `Bucket::put_object_stream` queues every part before sending any, so its
/// memory grows with the object; this does not. Returns the uploaded size,
/// after checking it against what the bucket reports.
pub async fn upload_stream<R: AsyncRead + Unpin>(bucket: &Bucket, reader: &mut R, key: &str) -> Result<u64> {
    let key = s3_key(key);
    let mut buf = Vec::with_capacity(PART_SIZE);
    read_part(reader, &mut buf).await?;
    let total = if buf.len() < PART_SIZE {
        bucket
            .put_object_with_content_type(key, &buf, CONTENT_TYPE)
            .await
            .with_context(|| format!("S3 upload of {key} failed"))?;
        buf.len() as u64
    } else {
        let upload = bucket
            .initiate_multipart_upload(key, CONTENT_TYPE)
            .await
            .with_context(|| format!("S3 multipart start for {key} failed"))?;
        let result: Result<u64> = async {
            let mut parts = Vec::new();
            let mut total = 0u64;
            while !buf.is_empty() {
                total += buf.len() as u64;
                let number = parts.len() as u32 + 1;
                let chunk = std::mem::replace(&mut buf, Vec::with_capacity(PART_SIZE));
                parts.push(
                    bucket
                        .put_multipart_chunk(chunk, key, number, &upload.upload_id, CONTENT_TYPE)
                        .await
                        .with_context(|| format!("S3 upload of part {number} of {key} failed"))?,
                );
                read_part(reader, &mut buf).await?;
            }
            bucket
                .complete_multipart_upload(key, &upload.upload_id, parts)
                .await
                .with_context(|| format!("S3 multipart completion for {key} failed"))?;
            Ok(total)
        }
        .await;
        if result.is_err() {
            let _ = bucket.abort_upload(key, &upload.upload_id).await;
        }
        result?
    };
    let (head, _) = bucket
        .head_object(key)
        .await
        .with_context(|| format!("S3 HEAD of {key} failed"))?;
    let stored = head.content_length.unwrap_or(-1);
    if stored != total as i64 {
        anyhow::bail!("S3 object {key} has {stored} bytes, uploaded {total}");
    }
    Ok(total)
}

pub async fn upload_file(bucket: &Bucket, local_path: &Path, key: &str) -> Result<u64> {
    let mut file = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to open file for upload: {e}"))?;
    upload_stream(bucket, &mut file, key).await
}

// Streamed: a restore must not need the whole object in memory.
pub async fn download_to<W: AsyncWrite + Send + Unpin>(bucket: &Bucket, key: &str, writer: &mut W) -> Result<()> {
    let status = bucket.get_object_to_writer(s3_key(key), writer).await?;
    if status != 200 {
        anyhow::bail!("S3 GET {key} returned status {status}");
    }
    writer.flush().await?;
    Ok(())
}

pub async fn download_file(bucket: &Bucket, key: &str, dest: &Path) -> Result<()> {
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| anyhow::anyhow!("failed to create download file: {e}"))?;
    download_to(bucket, key, &mut file).await
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
