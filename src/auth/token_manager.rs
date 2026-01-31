use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::auth::client::AuthClient;
use anyhow::{anyhow, Result};
use tracing::{error, info};

static TOKEN_PATH: &str = "tmp/token";

pub type SharedAccessToken = Arc<RwLock<String>>;

pub struct TokenManager {
    auth_client: AuthClient,
    access_token: SharedAccessToken,
    refresh_token: String,
}

impl TokenManager {
    pub fn new(auth_client: AuthClient, access_token: String, refresh_token: String) -> Result<Self> {
        Self::save_refresh_token(&refresh_token)?;

        Ok(Self {
            auth_client,
            access_token: Arc::new(RwLock::new(access_token)),
            refresh_token,
        })
    }

    /// 파일에서 refresh_token 로드 후 access_token 발급
    pub async fn load(mut auth_client: AuthClient) -> Result<Self> {
        let refresh_token = Self::load_refresh_token()?;

        let response = auth_client.refresh(refresh_token.clone()).await?;

        if !response.success {
            return Err(anyhow!("토큰 갱신 실패"));
        }

        Self::save_refresh_token(&response.refresh_token)?;
        let new_refresh_token = response.refresh_token;

        info!("저장된 토큰으로 인증 완료");

        Ok(Self {
            access_token: Arc::new(RwLock::new(response.access_token)),
            refresh_token: new_refresh_token,
            auth_client,
        })
    }

    /// access_token 갱신
    pub async fn refresh(&mut self) -> Result<()> {
        let response = self.auth_client.refresh(self.refresh_token.clone()).await?;

        if !response.success {
            return Err(anyhow!("토큰 갱신 실패"));
        }

        match self.access_token.write() {
            Ok(mut token) => *token = response.access_token,
            Err(e) => error!("access_token 쓰기 실패 (RwLock poisoned): {:?}", e),
        }

        // refresh token rotation 지원
        if !response.refresh_token.is_empty() {
            self.refresh_token = response.refresh_token;
            Self::save_refresh_token(&self.refresh_token)?;
        }

        info!("토큰 갱신 완료");
        Ok(())
    }

    /// AuthInterceptor에 전달할 SharedAccessToken 반환
    pub fn get_shared_token(&self) -> SharedAccessToken {
        Arc::clone(&self.access_token)
    }

    fn load_refresh_token() -> Result<String> {
        let path = Path::new(TOKEN_PATH);
        let token = fs::read_to_string(path)?;

        if token.trim().is_empty() {
            return Err(anyhow!("저장된 refresh_token이 비어 있음"));
        }

        Ok(token)
    }

    fn save_refresh_token(refresh_token: &str) -> Result<()> {
        let path = Path::new(TOKEN_PATH);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp = path.with_extension("tmp");
        fs::write(&tmp, refresh_token)?;
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
