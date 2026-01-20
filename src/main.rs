use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()>{
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    info!("Application started");

    Ok(())
}
