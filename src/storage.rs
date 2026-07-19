use crate::config::Config;
use anyhow::Result;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;

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

pub async fn upload_file(bucket: &Bucket, local_path: &Path, key: &str) -> Result<()> {
    let bytes = fs::read(local_path).await?;
    bucket.put_object(format!("/{key}"), &bytes).await?;
    Ok(())
}

pub async fn download_file(bucket: &Bucket, key: &str, dest: &Path) -> Result<()> {
    let response = bucket.get_object(format!("/{key}")).await?;
    let mut file = fs::File::create(dest).await?;
    file.write_all(response.bytes()).await?;
    Ok(())
}

pub async fn delete_object(bucket: &Bucket, key: &str) -> Result<()> {
    bucket.delete_object(format!("/{key}")).await?;
    Ok(())
}

pub async fn list_all(bucket: &Bucket) -> Result<Vec<RemoteObject>> {
    let pages = bucket.list("".to_string(), None).await?;
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
