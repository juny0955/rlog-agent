use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::auth::client::AuthClient;
use anyhow::{anyhow, Context, Result};
use tracing::{error, info};

static REFRESH_TOKEN_PATH: &str = "state/token";
static REFRESH_TOKEN: &str = "refresh_token";
static AGENT_UUID_PATH: &str = "state/agent_uuid";
static AGENT_UUID: &str = "agent_uuid";

pub type SharedAccessToken = Arc<RwLock<String>>;

pub struct TokenManager {
    auth_client: AuthClient,
    access_token: SharedAccessToken,
    refresh_token: String,
    agent_uuid: String,
    project_key: String,
}

impl TokenManager {
    pub fn new(
        auth_client: AuthClient,
        access_token: String,
        refresh_token: String,
        agent_uuid: String,
        project_key: String,
    ) -> Result<Self> {
        Self::save_refresh_token(&refresh_token)?;
        Self::save_agent_uuid(&agent_uuid)?;

        Ok(Self {
            auth_client,
            access_token: Arc::new(RwLock::new(access_token)),
            refresh_token,
            agent_uuid,
            project_key,
        })
    }

    /// 파일에서 refresh_token 로드 후 access_token 발급
    pub async fn load(mut auth_client: AuthClient, project_key: String) -> Result<Self> {
        let refresh_token = Self::load_refresh_token()?;
        let agent_uuid = Self::load_agent_uuid().unwrap_or_default();

        let (access_token, refresh_token, agent_uuid) =
            match auth_client.refresh(refresh_token.clone()).await {
                Ok(resp) if resp.success => {
                    Self::save_refresh_token(&resp.refresh_token)?;
                    info!("저장된 토큰으로 인증 완료");
                    (resp.access_token, resp.refresh_token, agent_uuid)
                }
                Ok(_) | Err(_) => {
                    info!("토큰 갱신 실패, 재등록 시도");
                    Self::do_register(&mut auth_client, &project_key).await?
                }
            };

        Ok(Self {
            access_token: Arc::new(RwLock::new(access_token)),
            refresh_token,
            auth_client,
            agent_uuid,
            project_key,
        })
    }

    /// access_token 갱신
    pub async fn refresh(&mut self) -> Result<()> {
        let response = match self.auth_client.refresh(self.refresh_token.clone()).await {
            Ok(resp) if resp.success => resp,
            Ok(_) | Err(_) => {
                info!("토큰 갱신 실패, 재등록 시도");
                return self.re_register().await;
            }
        };

        self.update_access_token(&response.access_token);

        // refresh token rotation 지원
        if !response.refresh_token.is_empty() {
            self.refresh_token = response.refresh_token;
            Self::save_refresh_token(&self.refresh_token)?;
        }

        Ok(())
    }

    /// 저장된 agent_uuid로 재등록
    async fn re_register(&mut self) -> Result<()> {
        let (access_token, refresh_token, agent_uuid) =
            Self::do_register(&mut self.auth_client, &self.project_key).await?;

        self.update_access_token(&access_token);
        self.refresh_token = refresh_token;
        self.agent_uuid = agent_uuid;

        Ok(())
    }

    /// 등록 수행 (신규/재등록 공통)
    async fn do_register(
        auth_client: &mut AuthClient,
        project_key: &str,
    ) -> Result<(String, String, String)> {
        let agent_uuid = Self::load_agent_uuid().ok();
        let response = auth_client
            .register(project_key, agent_uuid.as_deref())
            .await
            .context("등록 중 오류")?;

        if !response.success {
            return Err(anyhow!("등록 실패"));
        }

        Self::save_refresh_token(&response.refresh_token)?;
        Self::save_agent_uuid(&response.agent_uuid)?;
        info!("등록 완료");

        Ok((
            response.access_token,
            response.refresh_token,
            response.agent_uuid,
        ))
    }

    /// access_token 업데이트
    fn update_access_token(&self, new_token: &str) {
        match self.access_token.write() {
            Ok(mut token) => *token = new_token.to_string(),
            Err(e) => error!("access_token 쓰기 실패 (RwLock poisoned): {:?}", e),
        }
    }

    /// AuthInterceptor에 전달할 SharedAccessToken 반환
    pub fn get_shared_token(&self) -> SharedAccessToken {
        Arc::clone(&self.access_token)
    }

    fn load_refresh_token() -> Result<String> { Self::load_from_file(REFRESH_TOKEN_PATH, REFRESH_TOKEN) }

    fn save_refresh_token(refresh_token: &str) -> Result<()> { Self::save_to_file(REFRESH_TOKEN_PATH, refresh_token) }

    fn load_agent_uuid() -> Result<String> {
        Self::load_from_file(AGENT_UUID_PATH, AGENT_UUID)
    }

    fn save_agent_uuid(agent_uuid: &str) -> Result<()> { Self::save_to_file(AGENT_UUID_PATH, agent_uuid) }

    fn load_from_file(file_path: &str, name: &str) -> Result<String> {
        let path = Path::new(file_path);
        let content = fs::read_to_string(path)?;

        if content.trim().is_empty() {
            return Err(anyhow!("저장된 {}가 비어 있음", name));
        }

        Ok(content)
    }

    fn save_to_file(file_path: &str, content: &str) -> Result<()> {
        let path = Path::new(file_path);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp = path.with_extension("tmp");
        fs::write(&tmp, content)?;
        fs::rename(&tmp, path)?;

        // Unix 파일 권한 설정 (0600 - 소유자만 읽기/쓰기)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }
}
