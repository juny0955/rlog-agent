use crate::models::LogEvent;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio::time;
use tracing::info;

pub struct Forwarder {
    rx: Receiver<LogEvent>,
    batch_size: usize,
    flush_interval: Duration,
}

impl Forwarder {
    pub fn new(rx: Receiver<LogEvent>, batch_size: usize, flush_interval: u64) -> Self {
        Self { rx, batch_size, flush_interval: Duration::from_secs(flush_interval) }
    }

    pub async fn start(mut self) {
        let mut batch: Vec<LogEvent> = Vec::with_capacity(self.batch_size);
        let mut interval = time::interval(self.flush_interval);

        interval.tick().await;

        loop {
            tokio::select! {
                msg = self.rx.recv() => {
                    match msg {
                        Some(event) => {
                            info!("이벤트 수신");
                            batch.push(event);

                            if batch.len() >= self.batch_size {
                                self.flush(&mut batch).await;
                                interval.reset();
                            }
                        }
                        None => {
                            info!("채널 닫힘");
                            self.flush(&mut batch).await;
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    info!("인터벌 이벤트");
                    self.flush(&mut batch).await;
                }
            }
        }
    }

    async fn flush(&self, batch: &mut Vec<LogEvent>) {
        if batch.is_empty() { return; }

        batch.clear();
        info!("배치 전송 완료");
    }
}