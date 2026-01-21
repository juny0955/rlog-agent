mod settings;
mod models;
mod collector;

use crate::collector::Collector;
use crate::models::LogEvent;
use crate::settings::Settings;
use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::info;
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
    info!("settings loaded");
    info!("{} started", settings.name);

    let (collector_tx, mut collector_rx) = mpsc::channel::<LogEvent>(100);
    for source in settings.sources {
        let mut collector = Collector::new(
            collector_tx.clone(),
            source.label,
            source.path,
            &settings.timezone
        )?;

        tokio::spawn(async move {
            collector.start().await;
        });
    }
    
    while let Some(event) = collector_rx.recv().await {
        info!("event: {:?}", event);
    }

    Ok(())
}
