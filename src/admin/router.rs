//! Admin API 路由配置

use axum::{
    Router, middleware,
    routing::{delete, get, post},
};

use super::{
    handlers::{
        add_credential, create_api_key, create_group, delete_api_key, delete_credential,
        delete_group, export_credentials, force_refresh_token, get_all_credentials,
        get_credential_balance, get_credential_models, get_load_balancing_mode, list_api_keys,
        list_groups, list_usage_logs, reset_failure_count, rotate_api_key, set_account_groups,
        set_credential_disabled, set_credential_models, set_credential_priority,
        set_load_balancing_mode, update_api_key, update_group,
    },
    middleware::{AdminState, admin_auth_middleware},
};

/// 创建 Admin API 路由
///
/// # 端点
/// - `GET /credentials` - 获取所有凭据状态
/// - `POST /credentials` - 添加新凭据
/// - `POST /credentials/export` - 导出指定凭据
/// - `DELETE /credentials/:id` - 删除凭据
/// - `POST /credentials/:id/disabled` - 设置凭据禁用状态
/// - `POST /credentials/:id/priority` - 设置凭据优先级
/// - `POST /credentials/:id/reset` - 重置失败计数
/// - `POST /credentials/:id/refresh` - 强制刷新 Token
/// - `GET /credentials/:id/balance` - 获取凭据余额
/// - `GET /config/load-balancing` - 获取负载均衡模式
/// - `PUT /config/load-balancing` - 设置负载均衡模式
///
/// # 认证
/// 需要 Admin API Key 认证，支持：
/// - `x-api-key` header
/// - `Authorization: Bearer <token>` header
pub fn create_admin_router(state: AdminState) -> Router {
    Router::new()
        .route(
            "/credentials",
            get(get_all_credentials).post(add_credential),
        )
        .route("/credentials/export", post(export_credentials))
        .route("/credentials/{id}", delete(delete_credential))
        .route("/credentials/{id}/disabled", post(set_credential_disabled))
        .route("/credentials/{id}/priority", post(set_credential_priority))
        .route("/credentials/{id}/reset", post(reset_failure_count))
        .route("/credentials/{id}/refresh", post(force_refresh_token))
        .route(
            "/credentials/{id}/models",
            get(get_credential_models).put(set_credential_models),
        )
        .route("/credentials/{id}/balance", get(get_credential_balance))
        .route("/credentials/{id}/groups", post(set_account_groups))
        .route("/api-keys", get(list_api_keys).post(create_api_key))
        .route(
            "/api-keys/{id}",
            post(update_api_key).delete(delete_api_key),
        )
        .route("/api-keys/{id}/rotate", post(rotate_api_key))
        .route("/groups", get(list_groups).post(create_group))
        .route("/groups/{id}", post(update_group).delete(delete_group))
        .route("/usage-logs", get(list_usage_logs))
        .route(
            "/config/load-balancing",
            get(get_load_balancing_mode).put(set_load_balancing_mode),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ))
        .with_state(state)
}
