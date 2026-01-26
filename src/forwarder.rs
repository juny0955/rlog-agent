use std::mem;
use crate::models::LogEvent;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use prost_types::Timestamp;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time;
use tracing::info;
use uuid::Uuid;
use crate::agent::{Log, LogBatch};

pub struct Forwarder {
    rx: Receiver<LogEvent>,
    tx: Sender<LogBatch>,
    agent_id: String,
    agent_name: String,
    batch_size: usize,
    flush_interval: Duration,
}

impl Forwarder {
    pub fn new(
        rx: Receiver<LogEvent>,
        tx: Sender<LogBatch>,
        agent_id: String,
        agent_name: String,
        batch_size: usize,
        flush_interval: u64
    ) -> Self {
        Self {
            rx,
            tx,
            agent_id,
            agent_name,
            batch_size,
            flush_interval: Duration::from_secs(flush_interval)
        }
    }

    pub async fn start(mut self) {
        let mut logs: Vec<Log> = Vec::with_capacity(self.batch_size);
        let mut interval = time::interval(self.flush_interval);

        interval.tick().await;

        loop {
            tokio::select! {
                msg = self.rx.recv() => {
                    match msg {
                        Some(event) => {
                            logs.push(event_to_log(event));

                            if logs.len() >= self.batch_size {
                                self.flush(&mut logs).await;
                                interval.reset();
                            }
                        }
                        None => {
                            info!("모든 Collector 종료, 잔여 데이터 전송 중..");
                            self.flush(&mut logs).await;
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    self.flush(&mut logs).await;
                }
            }
        }

        info!("Forwarder 종료..");
    }

    async fn flush(&self, logs: &mut Vec<Log>) {
        if logs.is_empty() { return; }

        let send_logs = mem::take(logs);
        let batch_id = format!("{}-{}", self.agent_id, Uuid::new_v4());

        let batch = LogBatch {
            agent_id: self.agent_id.clone(),
            agent_name: self.agent_name.clone(),
            batch_id,
            send_at: Some(now()),
            logs: send_logs,
        };

        let _ = self.tx.send(batch).await;
    }
}

fn event_to_log(event: LogEvent) -> Log {
    Log {
        label: event.label,
        line: event.content,
        timestamp: Some(Timestamp {
            seconds: event.timestamp.timestamp(),
            nanos: event.timestamp.timestamp_subsec_nanos() as i32,
        }),
    }
}

fn now() -> Timestamp {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    Timestamp { seconds: now.as_secs() as i64, nanos: now.subsec_nanos() as i32 }
}
