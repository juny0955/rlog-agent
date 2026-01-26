mod settings;
mod models;
mod collector;
mod forwarder;

use crate::collector::Collector;
use crate::forwarder::Forwarder;
use crate::models::LogEvent;
use crate::settings::{Settings, SourceSettings};
use anyhow::{Context, Result};
use chrono_tz::Tz;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()>{
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    let settings = Settings::load_settings().context("설정 파일 로드 실패")?;
    info!("{} 시작 중..", settings.name);

    let shutdown = CancellationToken::new();

    let (collector_tx, collector_rx) = mpsc::channel::<LogEvent>(100);

    let collector_handles = start_collectors(
        collector_tx,
        settings.sources,
        settings.timezone,
        shutdown.child_token()
    ).await?;

    let mut forwarder_handle = start_forwarder(
        collector_rx,
        settings.batch_size,
        settings.flush_interval,
    ).await?;

    tokio::select! {
        _ = &mut forwarder_handle => {
            error!("Forwarder 채널 닫힘");
        }
        _ = signal::ctrl_c() => {
            info!("Ctrl+C 감지..");
        }
    }

    shutdown.cancel();

    for ch in collector_handles {
        let _ = ch.await;
    }

    let _ = forwarder_handle.await;

    info!("{} 정상 종료", settings.name);
    Ok(())
}

async fn start_collectors(
    tx: Sender<LogEvent>,
    source_settings: Vec<SourceSettings>,
    tz: Tz,
    shutdown: CancellationToken
) -> Result<Vec<JoinHandle<()>>>{
    let mut handles = Vec::new();

    for source in source_settings {
        let mut collector = Collector::new(tx.clone(), source, tz).await?;
        let child_shutdown = shutdown.child_token();

        handles.push(tokio::spawn(async move {
            collector.start(child_shutdown).await;
        }));
    }

    Ok(handles)
}

async fn start_forwarder(
    rx: Receiver<LogEvent>,
    batch_size: usize,
    flush_interval: u64,
) -> Result<JoinHandle<()>> {
    let forwarder = Forwarder::new(rx, batch_size, flush_interval);
    let handle = tokio::spawn(async move {
       forwarder.start().await;
    });

    Ok(handle)
}
