use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sysinfo::System;
use tokio::sync::RwLock;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;
use tonic::Code;
use tracing::{debug, error, info, warn};

use crate::auth::interceptor::AuthInterceptor;
use crate::auth::token_manager::TokenManager;
use crate::proto::health::health_service_client::HealthServiceClient;
use crate::proto::health::HeartbeatRequest;

type HealthClient = HealthServiceClient<tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>>;

static HEARTBEAT_INTERVAL_SECS: u64 = 10;

pub struct HealthReporter {
    client: HealthClient,
    token_manager: Arc<RwLock<TokenManager>>,
    system: System,
}

impl HealthReporter {
    pub fn new(
        channel: Channel,
        interceptor: AuthInterceptor,
        token_manager: Arc<RwLock<TokenManager>>,
    ) -> Self {
        let client = HealthServiceClient::with_interceptor(channel, interceptor);
        let system = System::new_all();

        Self {
            client,
            token_manager,
            system,
        }
    }

    pub async fn start(mut self, shutdown: CancellationToken) {
        info!("HealthReporter 시작");

        let mut ticker = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    info!("HealthReporter 종료");
                    break;
                }
                _ = ticker.tick() => {
                    if let Err(e) = self.send_heartbeat().await {
                        error!("Heartbeat 전송 실패: {}", e);
                    }
                }
            }
        }
    }

    async fn send_heartbeat(&mut self) -> Result<()> {
        self.system.refresh_cpu_all();
        self.system.refresh_memory();

        let cpu = self.system.global_cpu_usage() as f64;
        let memory = self.calculate_memory_usage();

        let request = HeartbeatRequest {
            timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
            cpu,
            memory,
        };

        match self.send_request(request.clone()).await {
            Ok(_) => {
                debug!(cpu = %cpu, memory = %memory, "Heartbeat 전송 완료");
                Ok(())
            }
            Err(status) if status.code() == Code::Unauthenticated => {
                warn!("토큰 만료, 재발급 시도");

                {
                    let mut tm = self.token_manager.write().await;
                    tm.refresh().await?;
                }

                self.send_request(request).await?;
                debug!("재시도 후 Heartbeat 전송 완료");
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn send_request(&mut self, request: HeartbeatRequest) -> Result<(), tonic::Status> {
        self.client.heartbeat(request).await?;
        Ok(())
    }

    fn calculate_memory_usage(&self) -> f64 {
        let total = self.system.total_memory();
        let used = self.system.used_memory();

        if total == 0 {
            return 0.0;
        }

        (used as f64 / total as f64) * 100.0
    }
}
