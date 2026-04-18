mod mirror;

use crate::utils::{determine_save_path, format_bytes, format_duration};
use anyhow::{anyhow, Result};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use reqwest::IntoUrl;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

pub async fn run(raw_url: &str, custom_name: Option<&str>) {
    let save_path = determine_save_path(raw_url, custom_name);
    let start = Instant::now();

    match download_smart(raw_url, &save_path).await {
        Ok(_) => {
            println!("Done in {}", format_duration(start.elapsed().as_secs()));
        }
        Err(_) => std::process::exit(1),
    }
}

async fn download_smart(url: &str, save_path: &Path) -> Result<()> {
    let url = mirror::resolve_url(url).await?;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()?;

    let total_size = client.head(&url).send().await?
        .content_length()
        .ok_or_else(|| anyhow!("No Content-Length"))?;

    // 检查文件是否已存在，如果存在则获取已下载的大小
    let downloaded_size = if save_path.exists() {
        tokio::fs::metadata(save_path).await?.len()
    } else {
        0
    };

    let pb = Arc::new(ProgressBar::new(total_size));
    pb.set_draw_target(ProgressDrawTarget::stdout_with_hz(10));
    pb.set_style(
        ProgressStyle::with_template("{msg}")
            .unwrap()
            .progress_chars("##"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    let start_time = Instant::now();
    let downloaded = Arc::new(AtomicU64::new(downloaded_size));
    let downloaded_clone = downloaded.clone();
    let pb_clone = pb.clone();

    // 启动进度更新任务
    let progress_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            let current_downloaded = downloaded_clone.load(Ordering::Relaxed);
            let elapsed = start_time.elapsed();
            let speed = if elapsed.as_secs_f64() > 0.0 {
                (current_downloaded - downloaded_size) as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            let percent = if total_size > 0 {
                (current_downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };
            let remaining = total_size.saturating_sub(current_downloaded);
            let eta = if speed > 0.0 {
                Duration::from_secs_f64(remaining as f64 / speed)
            } else {
                Duration::ZERO
            };
            let eta_str = if eta.as_secs() >= 3600 {
                format!(
                    "{:02}:{:02}:{:02}",
                    eta.as_secs() / 3600,
                    (eta.as_secs() % 3600) / 60,
                    eta.as_secs() % 60
                )
            } else {
                format!("{:02}:{:02}", eta.as_secs() / 60, eta.as_secs() % 60)
            };

            let msg = format!(
                "[{}/{}] {:.0}% {:.1}MB/s {}",
                format_bytes(current_downloaded),
                format_bytes(total_size),
                percent,
                speed / 1_000_000.0,
                eta_str
            );
            pb_clone.set_message(msg);

            if current_downloaded >= total_size {
                break;
            }
        }
    });

    // 多线程分块下载
    let chunk_size = 5 * 1024 * 1024; // 5MB per chunk
    let concurrent_downloads = 8; // 同时下载8个块
    let remaining = total_size - downloaded_size;
    let total_chunks = (remaining + chunk_size - 1) / chunk_size;

    // 使用信号量控制并发
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrent_downloads));
    let mut handles = vec![];

    for i in 0..total_chunks {
        let start = downloaded_size + i * chunk_size;
        let end = std::cmp::min(start + chunk_size, total_size);

        if start >= total_size {
            break;
        }

        let url_clone = url.clone();
        let save_path_clone = save_path.to_path_buf();
        let downloaded_clone = downloaded.clone();
        let semaphore_clone = semaphore.clone();

        let handle = tokio::spawn(async move {
            // 获取信号量许可
            let _permit = semaphore_clone.acquire().await.unwrap();

            download_chunk(
                &url_clone,
                &save_path_clone,
                start,
                end,
                downloaded_clone,
            ).await
        });

        handles.push(handle);
    }

    // 等待所有下载任务完成
    for handle in handles {
        handle.await??;
    }

    // 等待进度更新任务完成
    progress_task.await?;

    pb.finish();
    println!();
    Ok(())
}

async fn download_chunk(
    url: &str,
    save_path: &Path,
    start: u64,
    end: u64,
    downloaded: Arc<AtomicU64>,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()?;

    let response = client
        .get(url)
        .header("Range", format!("bytes={}-{}", start, end - 1))
        .send()
        .await?;

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(save_path)
        .await?;

    file.seek(std::io::SeekFrom::Start(start)).await?;

    let mut stream = response.bytes_stream();

    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }

    Ok(())
}


