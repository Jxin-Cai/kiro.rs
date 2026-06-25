//! Anthropic API 中间件

use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};

use crate::common::auth;
use crate::kiro::provider::{KiroProvider, RequestAuthContext};

use super::types::ErrorResponse;

/// 应用共享状态
#[derive(Clone)]
pub struct AppState {
    /// API 密钥
    pub api_key: String,
    /// Kiro Provider（可选，用于实际 API 调用）
    /// 内部使用 MultiTokenManager，已支持线程安全的多凭据管理
    pub kiro_provider: Option<Arc<KiroProvider>>,
    /// 是否开启非流式响应的 thinking 块提取
    pub extract_thinking: bool,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(api_key: impl Into<String>, extract_thinking: bool) -> Self {
        Self {
            api_key: api_key.into(),
            kiro_provider: None,
            extract_thinking,
        }
    }

    /// 设置 KiroProvider
    pub fn with_kiro_provider(mut self, provider: KiroProvider) -> Self {
        self.kiro_provider = Some(Arc::new(provider));
        self
    }
}

/// API Key 认证中间件
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let Some(key) = auth::extract_api_key(&request) else {
        let error = ErrorResponse::authentication_error();
        return (StatusCode::UNAUTHORIZED, Json(error)).into_response();
    };

    if auth::constant_time_eq(&key, &state.api_key) {
        let auth_context = if let Some(provider) = &state.kiro_provider {
            if let Some(store) = provider.token_manager().account_store() {
                match store.default_group_id().await {
                    Ok(group_id) => RequestAuthContext {
                        api_key_id: None,
                        group_id: Some(group_id),
                    },
                    Err(err) => {
                        tracing::warn!("查询默认账号组失败: {}", err);
                        RequestAuthContext::default()
                    }
                }
            } else {
                RequestAuthContext::default()
            }
        } else {
            RequestAuthContext::default()
        };
        request.extensions_mut().insert(auth_context);
        return next.run(request).await;
    }

    if let Some(provider) = &state.kiro_provider {
        if let Some(store) = provider.token_manager().account_store() {
            match store.find_api_key(&key).await {
                Ok(Some(record)) => {
                    request.extensions_mut().insert(RequestAuthContext {
                        api_key_id: Some(record.id),
                        group_id: record.group_id,
                    });
                    return next.run(request).await;
                }
                Ok(None) => {}
                Err(err) => tracing::warn!("查询客户端 API Key 失败: {}", err),
            }
        }
    }

    let error = ErrorResponse::authentication_error();
    (StatusCode::UNAUTHORIZED, Json(error)).into_response()
}

/// CORS 中间件层
///
/// **安全说明**：当前配置允许所有来源（Any），这是为了支持公开 API 服务。
/// 如果需要更严格的安全控制，请根据实际需求配置具体的允许来源、方法和头信息。
///
/// # 配置说明
/// - `allow_origin(Any)`: 允许任何来源的请求
/// - `allow_methods(Any)`: 允许任何 HTTP 方法
/// - `allow_headers(Any)`: 允许任何请求头
pub fn cors_layer() -> tower_http::cors::CorsLayer {
    use tower_http::cors::{Any, CorsLayer};

    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}
