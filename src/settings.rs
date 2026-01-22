use anyhow::Result;
use config::{Config, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub name: String,
    pub timezone: String,

    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    
    #[serde(default = "default_flush_interval")]
    pub flush_interval: u64,
    pub sources: Vec<SourceSettings>,
}

#[derive(Debug, Deserialize)]
pub struct SourceSettings {
    pub label: String,
    pub path: String,
}

fn default_batch_size() -> usize { 1000 }
fn default_flush_interval() -> u64 { 10 }

impl Settings {
    pub fn load_settings() -> Result<Self> {
        let settings = Config::builder()
            .add_source(File::with_name("config/agent.yaml"))
            .build()?
            .try_deserialize()?;

        Ok(settings)
    }
}
