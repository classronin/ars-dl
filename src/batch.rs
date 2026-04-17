use crate::utils::{ensure_dir, extract_filename};
use anyhow::Result;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use regex::Regex;
use reqwest::Client;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

const CONCURRENCY: usize = 10;
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

pub async fn run(raw_url: &str, save_folder: Option<&str>) {
    let (template, nums) = match parse_batch_url(raw_url) {
        Ok((t, n)) => (t, n),
        Err(e) => {
            eprintln!("Batch parse error: {}", e);
            std::process::exit(1);
        }
    };

    let folder = determine_folder(&template, save_folder);
    if let Err(e) = ensure_dir(&folder).await {
        eprintln!("Create folder failed: {}", e);
        std::process::exit(1);
    }

    let total = nums.len() as u64;
    let success = Arc::new(AtomicU32::new(0));
    let fail = Arc::new(AtomicU32::new(0));
    let skip = Arc::new(AtomicU32::new(0));
    let bytes_downloaded = Arc::new(AtomicU64::new(0));

    let client = Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("Failed to build client");

    let pb = ProgressBar::new(total);
    pb.set_draw_target(ProgressDrawTarget::stdout_with_hz(10));
    pb.set_style(
        ProgressStyle::with_template("{msg}")
            .unwrap()
            .progress_chars("##"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    let start = Instant::now();
    let pb_clone = pb.clone();
    let bytes_clone = bytes_downloaded.clone();
    let speed_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(200));
        loop {
            interval.tick().await;
            let completed = pb_clone.position();
            if completed >= total {
                break;
            }
            let percent = if total > 0 {
                (completed as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            let elapsed = start.elapsed();
            let bytes = bytes_clone.load(Ordering::Relaxed);
            let speed = if elapsed.as_secs_f64() > 0.0 {
                bytes as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            let msg = format!(
                "[{}/{}] {:.0}% {:.1}MB/s",
                completed,
                total,
                percent,
                speed / 1_000_000.0
            );
            pb_clone.set_message(msg);
        }
    });

    let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
    let mut handles = vec![];

    for num in nums {
        let url = format_template(&template, num);
        let file_name = extract_filename(&url).unwrap_or_else(|_| format!("file_{}", num));
        let save_path = folder.join(&file_name);

        let success = success.clone();
        let fail = fail.clone();
        let skip = skip.clone();
        let bytes_downloaded = bytes_downloaded.clone();
        let client = client.clone();
        let pb = pb.clone();
        let permit = semaphore.clone().acquire_owned().await.unwrap();

        let handle = tokio::spawn(async move {
            let _permit = permit;

            if save_path.exists() {
                skip.fetch_add(1, Ordering::Relaxed);
                pb.inc(1);
                return;
            }

            match download_file(&client, &url, &save_path).await {
                Ok(bytes) => {
                    success.fetch_add(1, Ordering::Relaxed);
                    bytes_downloaded.fetch_add(bytes, Ordering::Relaxed);
                }
                Err(_) => {
                    fail.fetch_add(1, Ordering::Relaxed);
                }
            }
            pb.inc(1);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.ok();
    }

    speed_task.abort();
    pb.finish();
    println!();

    let elapsed = start.elapsed();
    println!(
        "Succ:{} Fail:{} Skip:{} {}",
        success.load(Ordering::Relaxed),
        fail.load(Ordering::Relaxed),
        skip.load(Ordering::Relaxed),
        format_duration(elapsed.as_secs())
    );
}

async fn download_file(client: &Client, url: &str, path: &PathBuf) -> Result<u64> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    let len = bytes.len() as u64;
    let mut file = File::create(path).await?;
    file.write_all(&bytes).await?;
    Ok(len)
}

fn parse_batch_url(raw: &str) -> Result<(String, Vec<u32>), String> {
    let re = Regex::new(r"\[(\d+)-(\d+)(?::(\d+))?\]").unwrap();
    let caps = re.captures(raw).ok_or("invalid batch pattern")?;
    let start: u32 = caps[1].parse().unwrap();
    let end: u32 = caps[2].parse().unwrap();
    let step: u32 = caps.get(3).map(|m| m.as_str().parse().unwrap()).unwrap_or(1);
    if start > end || step == 0 {
        return Err("invalid range".to_string());
    }

    let width = caps[1].len();
    let template = re.replace(raw, format!("%0{}d", width)).to_string();
    let nums: Vec<u32> = (start..=end).step_by(step as usize).collect();
    Ok((template, nums))
}

fn format_template(template: &str, num: u32) -> String {
    if let Some(pos) = template.find('%') {
        let rest = &template[pos..];
        if rest.len() >= 4 && rest.chars().nth(1) == Some('0') {
            let width: usize = rest[2..rest.len() - 1].parse().unwrap_or(1);
            let num_str = format!("{:0width$}", num, width = width);
            return template.replace(&template[pos..pos + 4], &num_str);
        }
    }
    template.to_string()
}

fn determine_folder(template: &str, custom: Option<&str>) -> PathBuf {
    if let Some(c) = custom {
        PathBuf::from(c)
    } else {
        let url = url::Url::parse(template).unwrap();
        let path = std::path::Path::new(url.path());
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        PathBuf::from(stem.as_ref())
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