//! Admin API 业务逻辑服务

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::kiro::endpoint::EndpointRegistry;
use crate::kiro::model::credentials::KiroCredentials;
use crate::kiro::token_manager::MultiTokenManager;

use super::error::AdminServiceError;
use super::types::{
    AddCredentialRequest, AddCredentialResponse, BalanceResponse, CredentialModelOption,
    CredentialModelsResponse, CredentialStatusItem, CredentialsPagination, CredentialsQuery,
    CredentialsStatusResponse, LoadBalancingModeResponse, SetLoadBalancingModeRequest,
    SetSupportedModelsRequest, SuccessResponse,
};

/// 余额缓存过期时间（秒），5 分钟
const BALANCE_CACHE_TTL_SECS: i64 = 300;

/// 缓存的余额条目（含时间戳）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedBalance {
    /// 缓存时间（Unix 秒）
    cached_at: f64,
    /// 缓存的余额数据
    data: BalanceResponse,
}

/// Admin 服务
///
/// 封装所有 Admin API 的业务逻辑
pub struct AdminService {
    token_manager: Arc<MultiTokenManager>,
    balance_cache: Mutex<HashMap<u64, CachedBalance>>,
    cache_path: Option<PathBuf>,
    /// 端点注册表（用于 add_credential 校验）
    endpoint_registry: Arc<EndpointRegistry>,
}

impl AdminService {
    pub fn new(
        token_manager: Arc<MultiTokenManager>,
        endpoint_registry: Arc<EndpointRegistry>,
    ) -> Self {
        let cache_path = token_manager
            .cache_dir()
            .map(|d| d.join("kiro_balance_cache.json"));

        let balance_cache = Self::load_balance_cache_from(&cache_path);

        Self {
            token_manager,
            balance_cache: Mutex::new(balance_cache),
            cache_path,
            endpoint_registry,
        }
    }

    /// 获取所有凭据状态
    pub fn get_all_credentials(
        &self,
        query: CredentialsQuery,
    ) -> Result<CredentialsStatusResponse, AdminServiceError> {
        let snapshot = self.token_manager.snapshot();
        let default_endpoint = self.token_manager.config().default_endpoint.clone();

        let mut credentials: Vec<CredentialStatusItem> = snapshot
            .entries
            .into_iter()
            .map(|entry| CredentialStatusItem {
                id: entry.id,
                priority: entry.priority,
                disabled: entry.disabled,
                failure_count: entry.failure_count,
                is_current: entry.id == snapshot.current_id,
                expires_at: entry.expires_at,
                auth_method: entry.auth_method,
                has_profile_arn: entry.has_profile_arn,
                refresh_token_hash: entry.refresh_token_hash,
                api_key_hash: entry.api_key_hash,
                masked_api_key: entry.masked_api_key,
                email: entry.email,
                success_count: entry.success_count,
                last_used_at: entry.last_used_at.clone(),
                has_proxy: entry.has_proxy,
                proxy_url: entry.proxy_url,
                refresh_failure_count: entry.refresh_failure_count,
                disabled_reason: entry.disabled_reason,
                endpoint: entry.endpoint.unwrap_or_else(|| default_endpoint.clone()),
                supported_models: entry.supported_models,
            })
            .collect();

        let page_size = match query.page_size {
            50 | 100 | 200 => query.page_size,
            other => {
                return Err(AdminServiceError::InvalidCredential(format!(
                    "pageSize 必须是 50、100 或 200（当前: {}）",
                    other
                )));
            }
        };
        let page = query.page.max(1);

        if let Some(search) = query.search.as_ref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) {
            credentials.retain(|c| {
                c.id.to_string().contains(&search)
                    || c.email.as_deref().unwrap_or_default().to_lowercase().contains(&search)
                    || c.masked_api_key.as_deref().unwrap_or_default().to_lowercase().contains(&search)
                    || c.refresh_token_hash.as_deref().unwrap_or_default().to_lowercase().contains(&search)
                    || c.api_key_hash.as_deref().unwrap_or_default().to_lowercase().contains(&search)
                    || c.endpoint.to_lowercase().contains(&search)
            });
        }

        if let Some(status) = query.status.as_deref().filter(|s| *s != "all") {
            credentials.retain(|c| match status {
                "available" => !c.disabled,
                "current" => c.is_current,
                "problem" => c.failure_count > 0 || c.refresh_failure_count > 0 || c.disabled_reason.is_some(),
                "disabled" => c.disabled,
                _ => true,
            });
        }

        if let Some(auth_method) = query.auth_method.as_deref().filter(|s| *s != "all") {
            credentials.retain(|c| c.auth_method.as_deref() == Some(auth_method));
        }

        if let Some(endpoint) = query.endpoint.as_deref().filter(|s| *s != "all") {
            credentials.retain(|c| c.endpoint == endpoint);
        }

        let sort_key = query.sort_key.as_deref().unwrap_or("priority");
        credentials.sort_by(|a, b| {
            let ord = match sort_key {
                "status" => a.disabled.cmp(&b.disabled).then(a.failure_count.cmp(&b.failure_count)),
                "email" => a.email.cmp(&b.email).then(a.id.cmp(&b.id)),
                "failures" => (a.failure_count + a.refresh_failure_count)
                    .cmp(&(b.failure_count + b.refresh_failure_count))
                    .then(a.id.cmp(&b.id)),
                "success" => a.success_count.cmp(&b.success_count).then(a.id.cmp(&b.id)),
                "lastUsed" => a.last_used_at.cmp(&b.last_used_at).then(a.id.cmp(&b.id)),
                "id" => a.id.cmp(&b.id),
                _ => a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)),
            };
            if query.sort_direction.as_deref() == Some("desc") {
                ord.reverse()
            } else {
                ord
            }
        });

        let total_items = credentials.len();
        let total_pages = total_items.div_ceil(page_size).max(1);
        let page = page.min(total_pages);
        let start = (page - 1) * page_size;
        let credentials = credentials.into_iter().skip(start).take(page_size).collect();

        Ok(CredentialsStatusResponse {
            total: snapshot.total,
            available: snapshot.available,
            current_id: snapshot.current_id,
            credentials,
            pagination: CredentialsPagination {
                page,
                page_size,
                total_items,
                total_pages,
            },
        })
    }

    /// 导出指定凭据为批量导入兼容格式
    pub fn export_credentials(
        &self,
        ids: &[u64],
    ) -> Result<Vec<AddCredentialRequest>, AdminServiceError> {
        let credentials = self
            .token_manager
            .export_credentials(ids)
            .map_err(|e| self.classify_export_error(e))?;

        Ok(credentials
            .into_iter()
            .map(|credential| {
                let auth_method = if credential.is_api_key_credential() {
                    "api_key".to_string()
                } else {
                    credential.auth_method.clone().unwrap_or_else(|| {
                        if credential.client_id.is_some() && credential.client_secret.is_some() {
                            "idc".to_string()
                        } else {
                            "social".to_string()
                        }
                    })
                };

                AddCredentialRequest {
                    refresh_token: if credential.is_api_key_credential() {
                        None
                    } else {
                        credential.refresh_token
                    },
                    auth_method,
                    client_id: credential.client_id,
                    client_secret: credential.client_secret,
                    priority: credential.priority,
                    disabled: credential.disabled,
                    region: credential.region,
                    auth_region: credential.auth_region,
                    api_region: credential.api_region,
                    machine_id: credential.machine_id,
                    email: credential.email,
                    proxy_url: credential.proxy_url,
                    proxy_username: credential.proxy_username,
                    proxy_password: credential.proxy_password,
                    kiro_api_key: credential.kiro_api_key,
                    endpoint: credential.endpoint,
                    supported_models: credential.supported_models,
                }
            })
            .collect())
    }

    /// 设置凭据禁用状态
    pub fn set_disabled(&self, id: u64, disabled: bool) -> Result<(), AdminServiceError> {
        // 先获取当前凭据 ID，用于判断是否需要切换
        let snapshot = self.token_manager.snapshot();
        let current_id = snapshot.current_id;

        self.token_manager
            .set_disabled(id, disabled)
            .map_err(|e| self.classify_error(e, id))?;

        // 只有禁用的是当前凭据时才尝试切换到下一个
        if disabled && id == current_id {
            let _ = self.token_manager.switch_to_next();
        }
        Ok(())
    }

    /// 设置凭据优先级
    pub fn set_priority(&self, id: u64, priority: u32) -> Result<(), AdminServiceError> {
        self.token_manager
            .set_priority(id, priority)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 重置失败计数并重新启用
    pub fn reset_and_enable(&self, id: u64) -> Result<(), AdminServiceError> {
        self.token_manager
            .reset_and_enable(id)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 获取凭据余额（带缓存）
    pub async fn get_balance(&self, id: u64) -> Result<BalanceResponse, AdminServiceError> {
        // 先查缓存
        {
            let cache = self.balance_cache.lock();
            if let Some(cached) = cache.get(&id) {
                let now = Utc::now().timestamp() as f64;
                if (now - cached.cached_at) < BALANCE_CACHE_TTL_SECS as f64 {
                    tracing::debug!("凭据 #{} 余额命中缓存", id);
                    return Ok(cached.data.clone());
                }
            }
        }

        // 缓存未命中或已过期，从上游获取
        let balance = self.fetch_balance(id).await?;

        // 更新缓存
        {
            let mut cache = self.balance_cache.lock();
            cache.insert(
                id,
                CachedBalance {
                    cached_at: Utc::now().timestamp() as f64,
                    data: balance.clone(),
                },
            );
        }
        self.save_balance_cache();

        Ok(balance)
    }

    /// 从上游获取余额（无缓存）
    async fn fetch_balance(&self, id: u64) -> Result<BalanceResponse, AdminServiceError> {
        let usage = self
            .token_manager
            .get_usage_limits_for(id)
            .await
            .map_err(|e| self.classify_balance_error(e, id))?;

        let current_usage = usage.current_usage();
        let usage_limit = usage.usage_limit();
        let remaining = (usage_limit - current_usage).max(0.0);
        let usage_percentage = if usage_limit > 0.0 {
            (current_usage / usage_limit * 100.0).min(100.0)
        } else {
            0.0
        };

        Ok(BalanceResponse {
            id,
            subscription_title: usage.subscription_title().map(|s| s.to_string()),
            current_usage,
            usage_limit,
            remaining,
            usage_percentage,
            next_reset_at: usage.next_date_reset,
        })
    }

    /// 添加新凭据
    pub async fn add_credential(
        &self,
        req: AddCredentialRequest,
    ) -> Result<AddCredentialResponse, AdminServiceError> {
        // 校验端点名：未指定则默认合法，指定则必须已注册
        if let Some(ref name) = req.endpoint {
            if !self.endpoint_registry.contains(name) {
                let mut known: Vec<&str> = self.endpoint_registry.names();
                known.sort();
                return Err(AdminServiceError::InvalidCredential(format!(
                    "未知端点 \"{}\"，已注册端点: {:?}",
                    name, known
                )));
            }
        }

        // 构建凭据对象
        let email = req.email.clone();
        let new_cred = KiroCredentials {
            id: None,
            access_token: None,
            refresh_token: req.refresh_token,
            profile_arn: None,
            expires_at: None,
            auth_method: Some(req.auth_method),
            client_id: req.client_id,
            client_secret: req.client_secret,
            priority: req.priority,
            region: req.region,
            auth_region: req.auth_region,
            api_region: req.api_region,
            machine_id: req.machine_id,
            email: req.email,
            subscription_title: None, // 将在首次获取使用额度时自动更新
            proxy_url: req.proxy_url,
            proxy_username: req.proxy_username,
            proxy_password: req.proxy_password,
            disabled: req.disabled,
            kiro_api_key: req.kiro_api_key,
            endpoint: req.endpoint,
            supported_models: req.supported_models,
        };

        // 调用 token_manager 添加凭据
        let credential_id = self
            .token_manager
            .add_credential(new_cred)
            .await
            .map_err(|e| self.classify_add_error(e))?;

        Ok(AddCredentialResponse {
            success: true,
            message: format!("凭据添加成功，ID: {}", credential_id),
            credential_id,
            email,
        })
    }

    /// 使用当前账号从上游获取可用模型列表
    pub async fn get_credential_models(
        &self,
        id: u64,
    ) -> Result<CredentialModelsResponse, AdminServiceError> {
        let selected_models = self
            .token_manager
            .get_supported_models(id)
            .map_err(|e| self.classify_error(e, id))?;
        let models = self
            .token_manager
            .list_available_models_for(id)
            .await
            .map_err(|e| self.classify_balance_error(e, id))?
            .into_iter()
            .map(|option| CredentialModelOption {
                id: option.id,
                display_name: option.display_name,
                upstream_id: option.upstream_id,
                available: option.available,
                reason: option.reason,
            })
            .collect();

        Ok(CredentialModelsResponse {
            credential_id: id,
            selected_models,
            models,
        })
    }

    /// 设置当前账号可用模型列表
    pub fn set_supported_models(
        &self,
        id: u64,
        req: SetSupportedModelsRequest,
    ) -> Result<SuccessResponse, AdminServiceError> {
        let models = self
            .token_manager
            .set_supported_models(id, req.models)
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("不存在") {
                    AdminServiceError::NotFound { id }
                } else {
                    AdminServiceError::InvalidCredential(msg)
                }
            })?;
        let message = if models.is_empty() {
            format!("凭据 #{} 已设置为不限制模型", id)
        } else {
            format!("凭据 #{} 可用模型已更新（{} 个）", id, models.len())
        };
        Ok(SuccessResponse::new(message))
    }

    /// 删除凭据
    pub fn delete_credential(&self, id: u64) -> Result<(), AdminServiceError> {
        self.token_manager
            .delete_credential(id)
            .map_err(|e| self.classify_delete_error(e, id))?;

        // 清理已删除凭据的余额缓存
        {
            let mut cache = self.balance_cache.lock();
            cache.remove(&id);
        }
        self.save_balance_cache();

        Ok(())
    }

    /// 获取负载均衡模式
    pub fn get_load_balancing_mode(&self) -> LoadBalancingModeResponse {
        LoadBalancingModeResponse {
            mode: self.token_manager.get_load_balancing_mode(),
        }
    }

    /// 设置负载均衡模式
    pub fn set_load_balancing_mode(
        &self,
        req: SetLoadBalancingModeRequest,
    ) -> Result<LoadBalancingModeResponse, AdminServiceError> {
        // 验证模式值
        if req.mode != "priority" && req.mode != "balanced" {
            return Err(AdminServiceError::InvalidCredential(
                "mode 必须是 'priority' 或 'balanced'".to_string(),
            ));
        }

        self.token_manager
            .set_load_balancing_mode(req.mode.clone())
            .map_err(|e| AdminServiceError::InternalError(e.to_string()))?;

        Ok(LoadBalancingModeResponse { mode: req.mode })
    }

    /// 强制刷新指定凭据的 Token
    pub async fn force_refresh_token(&self, id: u64) -> Result<(), AdminServiceError> {
        self.token_manager
            .force_refresh_token_for(id)
            .await
            .map_err(|e| self.classify_balance_error(e, id))
    }

    // ============ 余额缓存持久化 ============

    fn load_balance_cache_from(cache_path: &Option<PathBuf>) -> HashMap<u64, CachedBalance> {
        let path = match cache_path {
            Some(p) => p,
            None => return HashMap::new(),
        };

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return HashMap::new(),
        };

        // 文件中使用字符串 key 以兼容 JSON 格式
        let map: HashMap<String, CachedBalance> = match serde_json::from_str(&content) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("解析余额缓存失败，将忽略: {}", e);
                return HashMap::new();
            }
        };

        let now = Utc::now().timestamp() as f64;
        map.into_iter()
            .filter_map(|(k, v)| {
                let id = k.parse::<u64>().ok()?;
                // 丢弃超过 TTL 的条目
                if (now - v.cached_at) < BALANCE_CACHE_TTL_SECS as f64 {
                    Some((id, v))
                } else {
                    None
                }
            })
            .collect()
    }

    fn save_balance_cache(&self) {
        let path = match &self.cache_path {
            Some(p) => p,
            None => return,
        };

        // 持有锁期间完成序列化和写入，防止并发损坏
        let cache = self.balance_cache.lock();
        let map: HashMap<String, &CachedBalance> =
            cache.iter().map(|(k, v)| (k.to_string(), v)).collect();

        match serde_json::to_string_pretty(&map) {
            Ok(json) => {
                if let Err(e) = std::fs::write(path, json) {
                    tracing::warn!("保存余额缓存失败: {}", e);
                }
            }
            Err(e) => tracing::warn!("序列化余额缓存失败: {}", e),
        }
    }

    // ============ 错误分类 ============

    /// 分类简单操作错误（set_disabled, set_priority, reset_and_enable）
    fn classify_error(&self, e: anyhow::Error, id: u64) -> AdminServiceError {
        let msg = e.to_string();
        if msg.contains("不存在") {
            AdminServiceError::NotFound { id }
        } else {
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类导出错误
    fn classify_export_error(&self, e: anyhow::Error) -> AdminServiceError {
        let msg = e.to_string();
        if msg.contains("凭据不存在") {
            AdminServiceError::InvalidCredential(msg)
        } else {
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类余额查询错误（可能涉及上游 API 调用）
    fn classify_balance_error(&self, e: anyhow::Error, id: u64) -> AdminServiceError {
        let msg = e.to_string();

        // 1. 凭据不存在
        if msg.contains("不存在") {
            return AdminServiceError::NotFound { id };
        }

        // 2. API Key 凭据不支持刷新：客户端请求错误，映射为 400
        if msg.contains("API Key 凭据不支持刷新") {
            return AdminServiceError::InvalidCredential(msg);
        }

        // 3. 上游服务错误特征：HTTP 响应错误或网络错误
        let is_upstream_error =
            // HTTP 响应错误（来自 refresh_*_token 的错误消息）
            msg.contains("凭证已过期或无效") ||
            msg.contains("权限不足") ||
            msg.contains("已被限流") ||
            msg.contains("服务器错误") ||
            msg.contains("Token 刷新失败") ||
            msg.contains("暂时不可用") ||
            // 网络错误（reqwest 错误）
            msg.contains("error trying to connect") ||
            msg.contains("connection") ||
            msg.contains("timeout") ||
            msg.contains("timed out");

        if is_upstream_error {
            AdminServiceError::UpstreamError(msg)
        } else {
            // 4. 默认归类为内部错误（本地验证失败、配置错误等）
            // 包括：缺少 refreshToken、refreshToken 已被截断、无法生成 machineId 等
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类添加凭据错误
    fn classify_add_error(&self, e: anyhow::Error) -> AdminServiceError {
        let msg = e.to_string();

        // 凭据验证失败（refreshToken 无效、格式错误等）
        let is_invalid_credential = msg.contains("缺少 refreshToken")
            || msg.contains("refreshToken 为空")
            || msg.contains("refreshToken 已被截断")
            || msg.contains("凭据已存在")
            || msg.contains("refreshToken 重复")
            || msg.contains("kiroApiKey 重复")
            || msg.contains("缺少 kiroApiKey")
            || msg.contains("kiroApiKey 为空")
            || msg.contains("凭证已过期或无效")
            || msg.contains("权限不足")
            || msg.contains("已被限流");

        if is_invalid_credential {
            AdminServiceError::InvalidCredential(msg)
        } else if msg.contains("error trying to connect")
            || msg.contains("connection")
            || msg.contains("timeout")
        {
            AdminServiceError::UpstreamError(msg)
        } else {
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类删除凭据错误
    fn classify_delete_error(&self, e: anyhow::Error, id: u64) -> AdminServiceError {
        let msg = e.to_string();
        if msg.contains("不存在") {
            AdminServiceError::NotFound { id }
        } else if msg.contains("只能删除已禁用的凭据") || msg.contains("请先禁用凭据")
        {
            AdminServiceError::InvalidCredential(msg)
        } else {
            AdminServiceError::InternalError(msg)
        }
    }
}
