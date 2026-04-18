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
        // 清理文件名中的非法字符（Windows 规则）
        let invalid_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
        let cleaned: String = c
            .chars()
            .map(|ch| if invalid_chars.contains(&ch) { '_' } else { ch })
            .collect();
        // 去除可能误输入的首尾引号（中文弯引号已在命令行阶段被处理，此处处理英文直引号残留）
        let cleaned = cleaned.trim_matches('"').trim_matches('\'');
        let mut p = PathBuf::from(cleaned);
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

/// 格式化字节数
pub fn format_bytes(b: u64) -> String {
    let b = b as f64;
    if b < 1_000.0 {
        format!("{:.0}B", b)
    } else if b < 1_000_000.0 {
        format!("{:.1}KB", b / 1_000.0)
    } else if b < 1_000_000_000.0 {
        format!("{:.1}MB", b / 1_000_000.0)
    } else {
        format!("{:.2}GB", b / 1_000_000_000.0)
    }
}

/// 格式化时长
pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let secs = secs % 60;
        format!("{}h{}m{}s", hours, mins, secs)
    }
}