use anyhow::Result;
use reqwest::Client;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
const MIRRORS: [&str; 2] = ["https://gh-proxy.com/", "https://ghfast.top/"];

pub async fn resolve_url(original: &str) -> Result<String> {
    if !original.starts_with("https://github.com") {
        return Ok(original.to_string());
    }

    let client = Client::builder().user_agent(USER_AGENT).build()?;
    for mirror in MIRRORS {
        let url = format!("{}{}", mirror, original);
        if client.head(&url).send().await.is_ok() {
            return Ok(url);
        }
    }
    Ok(original.to_string())
}