use tonic::service::Interceptor;
use tonic::{Request, Status};

use crate::auth::token_manager::SharedAccessToken;

#[derive(Clone)]
pub struct AuthInterceptor {
    access_token: SharedAccessToken,
}

impl AuthInterceptor {
    pub fn new(access_token: SharedAccessToken) -> Self {
        Self { access_token }
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let token = self.access_token
            .read()
            .map_err(|_| Status::internal("토큰 읽기 실패 (RwLock poisoned)"))?
            .clone();

        if token.is_empty() {
            return Err(Status::unauthenticated("토큰이 없습니다"));
        }

        let value = format!("Bearer {}", token)
            .parse()
            .map_err(|_| Status::internal("Invalid token format"))?;

        request.metadata_mut().insert("authorization", value);

        Ok(request)
    }
}
