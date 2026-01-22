mod settings;
mod models;
mod collector;
mod forwarder;

use crate::collector::Collector;
use crate::models::LogEvent;
use crate::settings::Settings;
use anyhow::{Context, Result};
use tokio::signal;
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use forwarder::Forwarder;

#[tokio::main]
async fn main() -> Result<()>{
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    let settings = Settings::load_settings().context("설정 파일 로드 실패")?;
    info!("settings loaded");
    info!("{} started", settings.name);

    let (collector_tx, collector_rx) = mpsc::channel::<LogEvent>(100);
    for source in settings.sources {
        let mut collector = Collector::new(collector_tx.clone(), source, settings.timezone.clone())?;
        tokio::spawn(async move {
            collector.start().await;
        });
    }
    drop(collector_tx);

    let forwarder = Forwarder::new(collector_rx, settings.batch_size, settings.flush_interval);

    tokio::select! {
        _ = forwarder.start() => {
            info!("종료");
        }
        _ = signal::ctrl_c() => {
            info!("Ctrl+C 종료");
        }
    }

    Ok(())
}
