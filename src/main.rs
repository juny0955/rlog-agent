mod settings;

use anyhow::{Context, Result};
use tracing::info;
use tracing_subscriber::EnvFilter;
use crate::settings::Settings;

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

    Ok(())
}
