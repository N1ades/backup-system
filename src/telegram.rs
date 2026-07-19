use crate::config::Config;
use anyhow::Result;

pub async fn notify(cfg: &Config, text: &str) -> Result<()> {
    let (Some(token), Some(chat_id)) = (&cfg.telegram_bot_token, &cfg.telegram_chat_id) else {
        return Ok(());
    };

    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let client = reqwest::Client::new();
    client
        .post(url)
        .form(&[("chat_id", chat_id.as_str()), ("text", text)])
        .send()
        .await?;
    Ok(())
}
