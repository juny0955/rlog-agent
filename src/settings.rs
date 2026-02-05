use crate::proto::auth::RegisterResponse;
use anyhow::Result;
use chrono_tz::Tz;
use config::{Config, File};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use tracing::info;

static CONFIG_PATH: &str = "config/agent.yaml";

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    pub server_addr: String,
    pub project_key: String,

    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    #[serde(default = "default_flush_interval")]
    pub flush_interval: u64,

    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u64,
    pub sources: Vec<SourceSettings>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceSettings {
    pub label: String,
    pub path: String,
}

fn default_batch_size() -> usize { 1000 }
fn default_flush_interval() -> u64 { 10 }
fn default_heartbeat_interval() -> u64 {
    30
}

impl Settings {
    pub fn load_settings() -> Result<Self> {
        let settings = Config::builder()
            .add_source(File::with_name(CONFIG_PATH))
            .build()?
            .try_deserialize()?;

        info!("설정 로드 완료");
        Ok(settings)
    }

    pub fn from_response(
        register_response: RegisterResponse,
        server_addr: String,
        project_key: String,
    ) -> Result<Self> {
        let sources = register_response
            .sources
            .into_iter()
            .filter(|s| s.enabled)
            .map(|s| SourceSettings {
                label: s.label,
                path: s.path,
            })
            .collect();

        Ok(Self {
            server_addr,
            project_key,
            batch_size: register_response.batch_size as usize,
            flush_interval: register_response.flush_interval_sec,
            heartbeat_interval: default_heartbeat_interval(),
            sources,
        })
    }

    pub fn save_settings(&self) -> Result<()> {
        let path = Path::new(CONFIG_PATH);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let yaml = serde_yaml::to_string(self)?;
        fs::write(path, yaml)?;

        Ok(())
    }
}
