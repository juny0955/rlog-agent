use anyhow::Result;
use config::{Config, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub name: String,
    pub timezone: String,
    pub sources: Vec<SourceSettings>,
}

#[derive(Debug, Deserialize)]
pub struct SourceSettings {
    pub label: String,
    pub path: String,
}

impl Settings {
    pub fn load_settings() -> Result<Self> {
        let settings = Config::builder()
            .add_source(File::with_name("config/agent.yaml"))
            .build()?
            .try_deserialize()?;

        Ok(settings)
    }
}