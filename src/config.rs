use anyhow::{Context, Result};
use chrono_tz::Tz;
use std::path::PathBuf;

pub struct Config {
    pub backup_root: PathBuf,
    pub download_dir: PathBuf,
    pub server_name: String,
    pub timezone: Tz,

    pub s3_endpoint: String,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_path_style: bool,

    pub max_file_size_mb: u64,
    pub max_dir_file_count: u64,
    pub retention_keep: usize,

    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<String>,
}

fn env_var(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing env var {key}"))
}

fn env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn load() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let timezone: Tz = env_var_or("TIMEZONE", "UTC")
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid TIMEZONE: {e}"))?;

        Ok(Config {
            backup_root: PathBuf::from(env_var_or("BACKUP_ROOT", "/srv")),
            download_dir: PathBuf::from(env_var_or("DOWNLOAD_DIR", "/downloads")),
            server_name: env_var("SERVER_NAME")?,
            timezone,

            s3_endpoint: env_var("S3_ENDPOINT")?,
            s3_region: env_var_or("S3_REGION", "auto"),
            s3_bucket: env_var("S3_BUCKET")?,
            s3_access_key: env_var("S3_ACCESS_KEY")?,
            s3_secret_key: env_var("S3_SECRET_KEY")?,
            s3_path_style: env_var_or("S3_PATH_STYLE", "true") == "true",

            max_file_size_mb: env_var_or("MAX_FILE_SIZE_MB", "10").parse()?,
            max_dir_file_count: env_var_or("MAX_DIR_FILE_COUNT", "2000").parse()?,
            retention_keep: env_var_or("RETENTION_KEEP", "10").parse()?,

            telegram_bot_token: std::env::var("TELEGRAM_BOT_TOKEN").ok(),
            telegram_chat_id: std::env::var("TELEGRAM_CHAT_ID").ok(),
        })
    }

    pub fn max_file_size_bytes(&self) -> u64 {
        self.max_file_size_mb * 1024 * 1024
    }
}
