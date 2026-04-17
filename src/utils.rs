use anyhow::Result;
use std::path::{Path, PathBuf};
use url::Url;

/// 从 URL 提取文件名
pub fn extract_filename(url_str: &str) -> Result<String> {
    let parsed = Url::parse(url_str)?;
    let path = parsed.path();
    path.split('/')
        .last()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("No filename found"))
}

/// 确保目录存在
pub async fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        tokio::fs::create_dir_all(path).await?;
    }
    Ok(())
}

/// 确定保存路径
pub fn determine_save_path(url: &str, custom: Option<&str>) -> PathBuf {
    let base = extract_filename(url).unwrap_or_else(|_| "download".to_string());
    if let Some(c) = custom {
        let mut p = PathBuf::from(c);
        if p.extension().is_none() {
            if let Some(ext) = Path::new(&base).extension() {
                p.set_extension(ext);
            }
        }
        p
    } else {
        PathBuf::from(base)
    }
}