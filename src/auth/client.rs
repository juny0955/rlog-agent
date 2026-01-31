use anyhow::Result;
use tonic::transport::Channel;
use tracing::info;

use crate::proto::auth::auth_service_client::AuthServiceClient;
use crate::proto::auth::{RefreshRequest, RefreshResponse, RegisterRequest, RegisterResponse};

pub struct AuthClient {
    client: AuthServiceClient<Channel>,
}

impl AuthClient {
    pub async fn connect(server_addr: &str) -> Result<Self> {
        let client = AuthServiceClient::connect(server_addr.to_string()).await?;
        Ok(Self { client })
    }

    pub async fn register(&mut self, project_key: &str, agent_uuid: Option<&str>) -> Result<RegisterResponse> {
        let hostname = gethostname::gethostname().to_string_lossy().into_owned();

        let os_info = os_info::get();
        let os = os_info.os_type().to_string();
        let os_version = os_info.version().to_string();

        let req = RegisterRequest {
            project_key: project_key.to_string(),
            hostname,
            os,
            os_version,
            agent_uuid: agent_uuid.map(|s| s.to_string()),
        };

        let response = self.client.register(req).await?.into_inner();
        info!("Agent 등록 완료");

        Ok(response)
    }

    pub async fn refresh(&mut self, refresh_token: String) -> Result<RefreshResponse> {
        let req = RefreshRequest { refresh_token };

        let response = self.client.refresh(req).await?.into_inner();
        info!("토큰 갱신 완료");

        Ok(response)
    }
}
