use crate::utils::determine_save_path;
use anyhow::{anyhow, Result};
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use reqwest::header::RANGE;
use reqwest::Client;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, SeekFrom};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
const CHUNK_THRESHOLD: u64 = 5 * 1024 * 1024;       // 5MB 以上才分块
const MAX_CONCURRENT_CHUNKS: usize = 8;             // 最大并发分块数
const MIRRORS: [&str; 2] = ["https://gh-proxy.com/", "https://ghfast.top/"];
const INDEPENDENT_CONNECTION_THRESHOLD: u64 = 50 * 1024 * 1024; // 50MB 以上启用独立连接

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
    let url = if url.starts_with("https://github.com") {
        try_mirrors(url).await?
    } else {
        url.to_string()
    };

    let client = build_optimized_client()?;
    let total_size = get_content_length(&client, &url).await?;

    if total_size >= CHUNK_THRESHOLD {
        let result = if total_size >= INDEPENDENT_CONNECTION_THRESHOLD {
            download_chunked_independent(&url, save_path, total_size).await
        } else {
            download_chunked_shared(&client, &url, save_path, total_size).await
        };
        if result.is_ok() {
            return Ok(());
        }
    }
    download_single_stream(&client, &url, save_path).await
}

async fn try_mirrors(original: &str) -> Result<String> {
    let client = build_optimized_client()?;
    for mirror in MIRRORS {
        let url = format!("{}{}", mirror, original);
        if client.head(&url).send().await.is_ok() {
            return Ok(url);
        }
    }
    Ok(original.to_string())
}

fn build_optimized_client() -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .tcp_keepalive(Duration::from_secs(60))
        .pool_max_idle_per_host(20)
        .http2_prior_knowledge()
        .build()
        .map_err(Into::into)
}

fn build_isolated_client() -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .tcp_keepalive(Duration::from_secs(60))
        .pool_max_idle_per_host(0) // 禁用连接池，每次新建 TCP 连接
        .http1_only()              // 强制 HTTP/1.1，避免 HTTP/2 单连接复用
        .build()
        .map_err(Into::into)
}

async fn get_content_length(client: &Client, url: &str) -> Result<u64> {
    let resp = client.head(url).send().await?;
    resp.content_length()
        .ok_or_else(|| anyhow!("No Content-Length"))
}

// ---------- 单流下载（降级方案）----------
async fn download_single_stream(client: &Client, url: &str, save_path: &Path) -> Result<()> {
    let (mut file, offset) = prepare_file(save_path).await?;
    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={}-", offset));
    }

    let mut response = request.send().await?;
    let status = response.status();
    if offset > 0 && status != reqwest::StatusCode::PARTIAL_CONTENT && status.is_success() {
        file.set_len(0).await?;
        file.seek(SeekFrom::Start(0)).await?;
        response = client.get(url).send().await?;
    }
    if !response.status().is_success() && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(anyhow!("HTTP {}", response.status()));
    }

    let total_size = response.content_length().map(|len| offset + len).unwrap_or(0);
    let pb = create_progress_bar(total_size);
    pb.set_position(offset);

    let mut downloaded = offset;
    let start_time = Instant::now();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
        update_speed(&pb, downloaded, start_time.elapsed());
    }
    file.flush().await?;
    pb.finish();
    println!();
    Ok(())
}

// ---------- 共享连接分块下载（5MB ~ 50MB）----------
async fn download_chunked_shared(
    client: &Client,
    url: &str,
    save_path: &Path,
    total_size: u64,
) -> Result<()> {
    use tokio::sync::Mutex;

    // 动态分块：确保至少 4 块，最多 16 块，单块不小于 1MB
    let min_chunks = 4;
    let max_chunks = 16;
    let min_chunk_size = 1 * 1024 * 1024;
    let dynamic_chunk_size = (total_size / min_chunks).max(min_chunk_size);
    let num_chunks = ((total_size + dynamic_chunk_size - 1) / dynamic_chunk_size)
        .min(max_chunks)
        .max(1);
    let chunk_size = (total_size + num_chunks - 1) / num_chunks;

    let temp_dir = std::env::temp_dir();
    let temp_prefix = save_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let temp_files: Vec<_> = (0..num_chunks)
        .map(|i| temp_dir.join(format!("ars-dl-{}.part{}", temp_prefix, i)))
        .collect();

    let client = Arc::new(client.clone());
    let pb = create_progress_bar(total_size);
    let downloaded = Arc::new(Mutex::new(0u64));
    let start_time = Instant::now();

    let pb_clone = pb.clone();
    let downloaded_clone = downloaded.clone();
    let speed_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(200));
        loop {
            interval.tick().await;
            let d = *downloaded_clone.lock().await;
            if d >= total_size {
                break;
            }
            update_speed(&pb_clone, d, start_time.elapsed());
        }
    });

    let download_result = stream::iter((0..num_chunks).zip(temp_files.iter().cloned()))
        .map(|(i, temp_path)| {
            let client = client.clone();
            let url = url.to_string();
            let downloaded = downloaded.clone();
            let pb = pb.clone();
            let start = i as u64 * chunk_size;
            let end = std::cmp::min(start + chunk_size - 1, total_size - 1);
            async move {
                let range = format!("bytes={}-{}", start, end);
                let resp = client
                    .get(&url)
                    .header(RANGE, &range)
                    .send()
                    .await?
                    .error_for_status()?;
                let mut file = File::create(&temp_path).await?;
                let mut stream = resp.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk?;
                    file.write_all(&chunk).await?;
                    let mut guard = downloaded.lock().await;
                    *guard += chunk.len() as u64;
                    pb.set_position(*guard);
                }
                file.flush().await?;
                Ok::<_, anyhow::Error>(())
            }
        })
        .buffer_unordered(MAX_CONCURRENT_CHUNKS)
        .collect::<Vec<_>>()
        .await;

    speed_task.abort();

    for res in download_result {
        if let Err(_) = res {
            for f in temp_files {
                let _ = tokio::fs::remove_file(f).await;
            }
            return Err(anyhow!("Chunk download failed"));
        }
    }

    pb.finish();
    println!();

    merge_temp_files(save_path, &temp_files).await
}

// ---------- 独立连接分块下载（>50MB，突破限速）----------
async fn download_chunked_independent(
    url: &str,
    save_path: &Path,
    total_size: u64,
) -> Result<()> {
    use tokio::sync::Mutex;

    // 动态分块：确保至少 4 块，最多 16 块，单块不小于 1MB
    let min_chunks = 4;
    let max_chunks = 16;
    let min_chunk_size = 1 * 1024 * 1024;
    let dynamic_chunk_size = (total_size / min_chunks).max(min_chunk_size);
    let num_chunks = ((total_size + dynamic_chunk_size - 1) / dynamic_chunk_size)
        .min(max_chunks)
        .max(1);
    let chunk_size = (total_size + num_chunks - 1) / num_chunks;

    let temp_dir = std::env::temp_dir();
    let temp_prefix = save_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let temp_files: Vec<_> = (0..num_chunks)
        .map(|i| temp_dir.join(format!("ars-dl-{}.part{}", temp_prefix, i)))
        .collect();

    let pb = create_progress_bar(total_size);
    let downloaded = Arc::new(Mutex::new(0u64));
    let start_time = Instant::now();

    let pb_clone = pb.clone();
    let downloaded_clone = downloaded.clone();
    let speed_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(200));
        loop {
            interval.tick().await;
            let d = *downloaded_clone.lock().await;
            if d >= total_size {
                break;
            }
            update_speed(&pb_clone, d, start_time.elapsed());
        }
    });

    let download_result = stream::iter((0..num_chunks).zip(temp_files.iter().cloned()))
        .map(|(i, temp_path)| {
            let url = url.to_string();
            let downloaded = downloaded.clone();
            let pb = pb.clone();
            let start = i as u64 * chunk_size;
            let end = std::cmp::min(start + chunk_size - 1, total_size - 1);
            async move {
                let client = build_isolated_client()?;
                let range = format!("bytes={}-{}", start, end);
                let resp = client
                    .get(&url)
                    .header(RANGE, &range)
                    .send()
                    .await?
                    .error_for_status()?;
                let mut file = File::create(&temp_path).await?;
                let mut stream = resp.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk?;
                    file.write_all(&chunk).await?;
                    let mut guard = downloaded.lock().await;
                    *guard += chunk.len() as u64;
                    pb.set_position(*guard);
                }
                file.flush().await?;
                Ok::<_, anyhow::Error>(())
            }
        })
        .buffer_unordered(MAX_CONCURRENT_CHUNKS)
        .collect::<Vec<_>>()
        .await;

    speed_task.abort();

    for res in download_result {
        if let Err(_) = res {
            for f in temp_files {
                let _ = tokio::fs::remove_file(f).await;
            }
            return Err(anyhow!("Chunk download failed"));
        }
    }

    pb.finish();
    println!();

    merge_temp_files(save_path, &temp_files).await
}

async fn merge_temp_files(final_path: &Path, temp_files: &[std::path::PathBuf]) -> Result<()> {
    let mut final_file = File::create(final_path).await?;
    for temp_path in temp_files {
        let mut part = File::open(temp_path).await?;
        tokio::io::copy(&mut part, &mut final_file).await?;
        let _ = tokio::fs::remove_file(temp_path).await;
    }
    Ok(())
}

async fn prepare_file(path: &Path) -> Result<(File, u64)> {
    if path.exists() {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .await?;
        let offset = file.metadata().await?.len();
        Ok((file, offset))
    } else {
        let file = File::create(path).await?;
        Ok((file, 0))
    }
}

fn create_progress_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_draw_target(ProgressDrawTarget::stdout_with_hz(10));
    pb.set_style(
        ProgressStyle::with_template("{msg}")
            .unwrap()
            .progress_chars("##"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

fn update_speed(pb: &ProgressBar, downloaded: u64, elapsed: Duration) {
    let speed = if elapsed.as_secs_f64() > 0.0 {
        downloaded as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };
    let total = pb.length().unwrap_or(0);
    let percent = if total > 0 {
        (downloaded as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    let msg = format!(
        "[{}/{}] {:.0}% {:.1}MB/s",
        format_bytes(downloaded),
        format_bytes(total),
        percent,
        speed / 1_000_000.0
    );
    pb.set_message(msg);
}

fn format_bytes(b: u64) -> String {
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

fn format_duration(secs: u64) -> String {
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