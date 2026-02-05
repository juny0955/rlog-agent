mod auth;
mod collector;
mod forwarder;
mod health;
mod models;
mod proto;
mod settings;
mod streamer;

use std::sync::Arc;

use crate::auth::client::AuthClient;
use crate::auth::interceptor::AuthInterceptor;
use crate::auth::token_manager::TokenManager;
use crate::collector::Collector;
use crate::forwarder::Forwarder;
use crate::health::HealthReporter;
use crate::models::LogEvent;
use crate::proto::log::LogBatch;
use crate::settings::{Settings, SourceSettings};
use crate::streamer::Streamer;
use anyhow::{anyhow, bail, Result};
use tokio::signal;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

static ENV_SERVER_ADDR: &str = "SERVER_ADDR";
static ENV_PROJECT_KEY: &str = "PROJECT_KEY";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Agent 시작 중..");
    let (settings, token_manager) = load_settings_and_auth().await?;

    let shutdown = CancellationToken::new();
    let token_manager = Arc::new(RwLock::new(token_manager));

    let (collector_tx, collector_rx) = mpsc::channel::<LogEvent>(100);
    let (streamer_tx, streamer_rx) = mpsc::channel::<LogBatch>(1000);

    let collector_handles = start_collectors(
        collector_tx,
        settings.sources,
        shutdown.child_token(),
    )
    .await?;

    let forwarder_handle = start_forwarder(
        collector_rx,
        streamer_tx,
        settings.batch_size,
        settings.flush_interval,
    )
    .await?;

    let streamer_handle = start_streamer(
        streamer_rx,
        &settings.server_addr,
        Arc::clone(&token_manager),
    )
    .await?;

    let health_handle = start_health_reporter(
        &settings.server_addr,
        Arc::clone(&token_manager),
        shutdown.child_token(),
    )
    .await?;

    tokio::select! {
        _ = forwarder_handle => {
            error!("Forwarder 종료");
        }
        _ = streamer_handle => {
            error!("Streamer 종료");
        }
        _ = health_handle => {
            error!("HealthReporter 종료");
        }
        _ = signal::ctrl_c() => {
            info!("Ctrl+C 감지..");
        }
    }

    shutdown.cancel();

    for ch in collector_handles {
        if let Err(e) = ch.await {
            error!("Collector 태스크 종료 오류: {:?}", e);
        }
    }

    info!("Agent 정상 종료");
    Ok(())
}

async fn start_collectors(
    tx: Sender<LogEvent>,
    source_settings: Vec<SourceSettings>,
    shutdown: CancellationToken,
) -> Result<Vec<JoinHandle<()>>> {
    let mut handles = Vec::new();

    for source in source_settings {
        let mut collector = Collector::new(tx.clone(), source).await?;
        let child_shutdown = shutdown.child_token();

        handles.push(tokio::spawn(async move {
            collector.start(child_shutdown).await;
        }));
    }

    Ok(handles)
}

async fn load_settings_and_auth() -> Result<(Settings, TokenManager)> {
    match Settings::load_settings() {
        Ok(settings) => {
            // 설정 파일 있음 -> 저장된 토큰으로 인증
            let auth_client = AuthClient::connect(&settings.server_addr).await?;
            let token_manager =
                TokenManager::load(auth_client, settings.project_key.clone()).await?;
            info!("설정 및 토큰 로드 완료");
            Ok((settings, token_manager))
        }
        Err(_) => {
            // 설정 파일 없음 -> 신규 등록
            warn!("설정파일 로드 실패, 에이전트 등록 수행");
            let (server_addr, project_key) = get_env()?;

            let mut auth_client = AuthClient::connect(&server_addr).await?;
            let response = auth_client.register(&project_key, None).await?;

            if !response.success {
                bail!("에이전트 등록 실패");
            }

            let settings = Settings::from_response(
                response.clone(),
                server_addr.clone(),
                project_key.clone(),
            )?;
            settings.save_settings()?;

            let auth_client_for_token = AuthClient::connect(&server_addr).await?;
            let token_manager = TokenManager::new(
                auth_client_for_token,
                response.access_token,
                response.refresh_token,
                response.agent_uuid,
                project_key,
            )?;

            info!("에이전트 등록 및 설정 저장 완료");
            Ok((settings, token_manager))
        }
    }
}

async fn start_forwarder(
    rx: Receiver<LogEvent>,
    tx: Sender<LogBatch>,
    batch_size: usize,
    flush_interval: u64,
) -> Result<JoinHandle<()>> {
    let forwarder = Forwarder::new(rx, tx, batch_size, flush_interval);
    let handle = tokio::spawn(async move {
        forwarder.start().await;
    });

    Ok(handle)
}

async fn start_streamer(
    rx: Receiver<LogBatch>,
    server_addr: &str,
    token_manager: Arc<RwLock<TokenManager>>,
) -> Result<JoinHandle<()>> {
    let channel = Channel::from_shared(server_addr.to_string())?
        .connect()
        .await?;

    let shared_token = {
        let tm = token_manager.read().await;
        tm.get_shared_token()
    };

    let interceptor = AuthInterceptor::new(shared_token);
    let streamer = Streamer::new(rx, channel, interceptor, token_manager);

    let handle = tokio::spawn(async move {
        streamer.start().await;
    });

    Ok(handle)
}

async fn start_health_reporter(
    server_addr: &str,
    token_manager: Arc<RwLock<TokenManager>>,
    shutdown: CancellationToken,
) -> Result<JoinHandle<()>> {
    let channel = Channel::from_shared(server_addr.to_string())?
        .connect()
        .await?;

    let shared_token = {
        let tm = token_manager.read().await;
        tm.get_shared_token()
    };

    let interceptor = AuthInterceptor::new(shared_token);
    let reporter = HealthReporter::new(channel, interceptor, token_manager);

    let handle = tokio::spawn(async move {
        reporter.start(shutdown).await;
    });

    Ok(handle)
}

fn get_env() -> Result<(String, String)> {
    let server_addr = std::env::var(ENV_SERVER_ADDR)
        .map_err(|_| anyhow!("SERVER_ADDR 환경 변수를 찾을 수 없음"))?;

    let project_key = std::env::var(ENV_PROJECT_KEY)
        .map_err(|_| anyhow!("PROJECT_KEY 환경 변수를 찾을 수 없음"))?;

    if server_addr.trim().is_empty() {
        bail!("SERVER_ADDR 환경 변수가 비어 있음")
    }

    if project_key.trim().is_empty() {
        bail!("PROJECT_KEY 환경 변수가 비어 있음")
    }

    Ok((server_addr, project_key))
}
