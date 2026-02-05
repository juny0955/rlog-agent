use crate::models::LogEvent;
use crate::settings::SourceSettings;
use anyhow::{Context, Result};
use chrono::Utc;
use notify::{Watcher, recommended_watcher};
use std::fs::Metadata;
use std::io::SeekFrom;
use std::path::PathBuf;
use tokio::fs::{File, metadata};
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use tokio::sync::mpsc::{self, Sender};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

pub struct Collector {
    tx: Sender<LogEvent>,
    label: String,
    path: PathBuf,
    reader: BufReader<File>,
    position: u64,
    file_id: u64,
}

impl Collector {
    pub async fn new(tx: Sender<LogEvent>, source: SourceSettings) -> Result<Self> {
        let path = PathBuf::from(source.path);

        let (reader, position, file_id) = open_file(&path, true)
            .await
            .with_context(|| format!("파일 열기 실패: {}", source.label))?;

        Ok(Self {
            tx,
            label: source.label,
            path,
            reader,
            position,
            file_id,
        })
    }

    pub async fn start(&mut self, shutdown: CancellationToken) {
        let (watcher_tx, mut watcher_rx) = mpsc::channel::<()>(1);

        let mut watcher = recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res
                && event.kind.is_modify()
            {
                let _ = watcher_tx.try_send(());
            }
        })
        .expect("Watcher 생성 실패");

        if let Err(e) = watcher.watch(&self.path, notify::RecursiveMode::NonRecursive) {
            error!("{} 파일 감지 설정 중 오류 {}", self.label, e);
            return;
        }

        info!("{} 파일 감지 시작", self.label);

        let mut line = String::new();
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    let _ = self.read_line_to_send(&mut line).await;
                    break;
                }
                recv = watcher_rx.recv() => {
                    match recv {
                        Some(()) => {
                            if let Err(e) = self.read_line_to_send(&mut line).await {
                                warn!("{} ({}) 파일 읽기 중 오류: {}", self.label, self.path.display(), e);
                            }
                        }
                        None => break,
                    }
                }
            }
        }

        info!("{} Collector 종료..", self.label);
    }

    async fn read_line_to_send(&mut self, line: &mut String) -> Result<()> {
        loop {
            let read_bytes = self
                .reader
                .read_line(line)
                .await
                .context("라인 읽기 실패")?;

            if read_bytes == 0 {
                if let Ok(meta) = metadata(&self.path).await
                    && self.check_rotation_or_truncate(meta).await?
                {
                    continue;
                }
                break;
            }

            // 개행문자 없으면 무시
            if !line.ends_with('\n') {
                break;
            }

            self.send_event(line).await?;
            self.position += read_bytes as u64;
            line.clear();
        }

        Ok(())
    }

    async fn check_rotation_or_truncate(&mut self, meta: Metadata) -> Result<bool> {
        let current_file_id = get_file_id(&meta);
        let current_len = meta.len();

        if current_file_id != self.file_id {
            info!("{} Rotation 감지", self.label);
            self.reopen(false).await?;
            return Ok(true);
        }

        if current_len < self.position {
            info!(
                "{} Truncation  감지 {} -> {}",
                self.label, self.position, current_len
            );
            self.reopen(true).await?;
            return Ok(true);
        }

        Ok(false)
    }

    async fn send_event(&self, line: &str) -> Result<()> {
        if line.trim().is_empty() {
            return Ok(());
        }

        let event = LogEvent {
            label: self.label.clone(),
            content: line.trim_end().to_string(),
            timestamp: Utc::now(),
        };

        self.tx.send(event).await.context("메세지 채널 닫힘")?;

        Ok(())
    }

    async fn reopen(&mut self, seek_to_end: bool) -> Result<()> {
        let (reader, position, file_id) = open_file(&self.path, seek_to_end)
            .await
            .context("파일 재열기 실패")?;

        self.reader = reader;
        self.position = position;
        self.file_id = file_id;

        Ok(())
    }
}

async fn open_file(path: &PathBuf, seek_to_end: bool) -> Result<(BufReader<File>, u64, u64)> {
    let file = File::open(path).await.context("파일 열기 실패")?;

    let meta = file.metadata().await.context("파일 메타데이터 읽기 실패")?;

    let position = if seek_to_end { meta.len() } else { 0 };
    let file_id = get_file_id(&meta);

    let mut reader = BufReader::new(file);
    reader
        .seek(SeekFrom::Start(position))
        .await
        .context("파일 포인터 이동 실패")?;

    Ok((reader, position, file_id))
}

fn get_file_id(meta: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        meta.ino()
    }

    #[cfg(windows)]
    {
        // NOTE: file_index()가 unstable 이라서 creation_time 사용
        // meta.file_index().unwrap_or(0)
        meta.creation_time()
    }
}
