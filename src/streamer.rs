use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc::Receiver};
use tonic::Code;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Channel;
use tracing::{error, info, warn};

use crate::auth::interceptor::AuthInterceptor;
use crate::auth::token_manager::TokenManager;
use crate::proto::log::LogBatch;
use crate::proto::log::log_service_client::LogServiceClient;

type LogClient = LogServiceClient<InterceptedService<Channel, AuthInterceptor>>;

pub struct Streamer {
    rx: Receiver<LogBatch>,
    client: LogClient,
    token_manager: Arc<RwLock<TokenManager>>,
}

impl Streamer {
    pub fn new(
        rx: Receiver<LogBatch>,
        channel: Channel,
        interceptor: AuthInterceptor,
        token_manager: Arc<RwLock<TokenManager>>,
    ) -> Self {
        let client = LogServiceClient::with_interceptor(channel, interceptor);
        Self {
            rx,
            client,
            token_manager,
        }
    }

    pub async fn start(mut self) {
        info!("Streamer 시작");

        while let Some(batch) = self.rx.recv().await {
            if let Err(e) = self.send_with_retry(batch).await {
                error!("로그 전송 실패: {}", e);
            }
        }

        info!("Streamer 종료");
    }

    async fn send_with_retry(&mut self, batch: LogBatch) -> Result<()> {
        let batch_id = batch.batch_id.clone();
        let log_count = batch.logs.len();

        match self.send_batch(batch.clone()).await {
            Ok(_) => {
                info!(batch_id = %batch_id, count = log_count, "로그 전송 완료");
                Ok(())
            }
            Err(status) if status.code() == Code::Unauthenticated => {
                warn!("토큰 만료, 재발급 시도");

                {
                    let mut tm = self.token_manager.write().await;
                    tm.refresh().await?;
                }

                self.send_batch(batch).await?;
                info!(batch_id = %batch_id, count = log_count, "재시도 후 로그 전송 완료");
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn send_batch(&mut self, batch: LogBatch) -> Result<(), tonic::Status> {
        let stream = tokio_stream::once(batch);
        self.client.send(stream).await?;
        Ok(())
    }
}
