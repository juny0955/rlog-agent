use crate::models::LogEvent;
use crate::settings::SourceSettings;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use chrono_tz::Tz;
use notify::{recommended_watcher, Watcher};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use tokio::sync::mpsc::{self, Sender};
use tracing::{error, info, trace};

pub struct Collector {
    tx: Sender<LogEvent>,
    label: String,
    path: PathBuf,
    tz: Tz,
    position: u64,
}

impl Collector {
    pub fn new(tx: Sender<LogEvent>, source: SourceSettings, tz: String) -> Result<Self> {
        let path = PathBuf::from(source.path);
        let file = File::open(&path)
            .with_context(|| format!("파일 열기 실패: {}", source.label))?;

        let position = file.metadata()
            .with_context(|| format!("파일 메타데이터 읽기 실패: {}", source.label))?
            .len();

        let tz = tz.parse::<Tz>()?;

        Ok(Self { tx, label: source.label, path, tz, position })
    }

    pub async fn start(&mut self) {
        let (watcher_tx, mut watcher_rx) = mpsc::channel::<()>(1);

        let mut watcher = recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() { let _ = watcher_tx.try_send(()); }
            }
        }).expect("watcher 생성 실패");

        if let Err(e) = watcher.watch(&self.path, notify::RecursiveMode::NonRecursive) {
            error!("{} 파일 감지 설정 중 오류 {}", self.label, e);
            return;
        }

        info!("{} 파일 감지 시작", self.label);

        let mut line = String::new();
        while let Some(_) = watcher_rx.recv().await {
            trace!("{} 파일 변경 감지", self.label);

            if let Err(e) = self.read_line_to_send(&mut line).await {
                error!("{} ({}) 파일 읽기 중 오류 {}", self.label, self.path.display(), e);
            }
        }
    }

    async fn read_line_to_send(&mut self, line: &mut String) -> Result<()> {
        let file = File::open(&self.path)
            .context("파일 열기 실패")?;

        let current_position = file.metadata()
            .context("파일 메타데이터 읽기 실패")?
            .len();

        if current_position < self.position {
            info!("{} 로테이션 감지", self.label);
            self.position = 0;
        }

        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(self.position))
            .context("파일 포인터 이동 실패")?;

        loop {
            let read_bytes = reader.read_line(line)
                .context("라인 읽기 실패")?;

            if read_bytes == 0 { break; }

            if !line.ends_with('\n') {
                line.clear();
                break;
            }

            if !line.trim().is_empty() {
                let event = LogEvent {
                    label: self.label.clone(),
                    content: line.trim_end().to_string(),
                    timestamp: Utc::now().with_timezone(&self.tz),
                };

                if let Err(e) = self.tx.send(event).await {
                    error!("{} 로그 전송 실패: {}", self.label, e);
                    bail!("메세지 채널 닫힘");
                }
            }

            self.position = reader.stream_position()
                .context("파일 포인터 업데이트 실패")?;

            line.clear();
        }

        Ok(())
    }
}