//! Kiro API Provider
//!
//! 核心组件，负责与 Kiro API 通信
//! 支持流式和非流式请求
//! 支持多凭据故障转移和重试
//! 支持按凭据级 endpoint 切换不同 Kiro API 端点

use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::http_client::{ProxyConfig, build_client};
use crate::kiro::account_store::UsageLogDraft;
use crate::kiro::endpoint::{EndpointErrorKind, KiroRequest};
use crate::kiro::model::credentials::{KiroCredentials, TempUnschedulableRule};
use crate::kiro::token_manager::MultiTokenManager;

// 注：KiroProvider 本身不再直接持有 EndpointRegistry —— 重试循环通过 ctx.endpoint() 访问。
// TokenManager 单向持有 EndpointRegistry，并在 acquire_context* 构造 CallContext 时预解析。
use crate::model::config::TlsBackend;
use parking_lot::Mutex;

/// 每个凭据的最大重试次数
const MAX_RETRIES_PER_CREDENTIAL: usize = 3;

/// 总重试次数硬上限（避免无限重试）
const MAX_TOTAL_RETRIES: usize = 9;

#[derive(Debug, Clone, Default)]
pub struct RequestAuthContext {
    pub api_key_id: Option<i64>,
    pub group_id: Option<i64>,
}

/// Kiro API Provider
///
/// 核心组件，负责与 Kiro API 通信
/// 支持多凭据故障转移和重试机制
/// 按凭据 `endpoint` 字段选择 [`KiroEndpoint`] 实现
pub struct KiroProvider {
    token_manager: Arc<MultiTokenManager>,
    /// 全局代理配置（用于凭据无自定义代理时的回退）
    global_proxy: Option<ProxyConfig>,
    /// Client 缓存：key = effective proxy config, value = reqwest::Client
    /// 不同代理配置的凭据使用不同的 Client，共享相同代理的凭据复用 Client
    client_cache: Mutex<HashMap<Option<ProxyConfig>, Client>>,
    /// TLS 后端配置
    tls_backend: TlsBackend,
}

impl KiroProvider {
    /// 创建带代理配置的 KiroProvider 实例
    ///
    /// 端点注册表通过 `TokenManager` 间接注入：Provider 的重试循环从 `ctx.endpoint()`
    /// 取端点实现，无需自身持有 `EndpointRegistry`。
    ///
    /// # Arguments
    /// * `token_manager` - 多凭据 Token 管理器（已持有 endpoint_registry）
    /// * `proxy` - 全局代理配置
    pub fn with_proxy(token_manager: Arc<MultiTokenManager>, proxy: Option<ProxyConfig>) -> Self {
        let tls_backend = token_manager.config().tls_backend;
        // 预热：构建全局代理对应的 Client
        let initial_client =
            build_client(proxy.as_ref(), 720, tls_backend).expect("创建 HTTP 客户端失败");
        let mut cache = HashMap::new();
        cache.insert(proxy.clone(), initial_client);

        Self {
            token_manager,
            global_proxy: proxy,
            client_cache: Mutex::new(cache),
            tls_backend,
        }
    }

    /// 根据凭据的代理配置获取（或创建并缓存）对应的 reqwest::Client
    fn client_for(&self, credentials: &KiroCredentials) -> anyhow::Result<Client> {
        let effective = credentials.effective_proxy(self.global_proxy.as_ref());
        let mut cache = self.client_cache.lock();
        if let Some(client) = cache.get(&effective) {
            return Ok(client.clone());
        }
        let client = build_client(effective.as_ref(), 720, self.tls_backend)?;
        cache.insert(effective, client.clone());
        Ok(client)
    }

    pub(crate) fn token_manager(&self) -> Arc<MultiTokenManager> {
        Arc::clone(&self.token_manager)
    }

    /// 发送非流式 API 请求
    ///
    /// 支持多凭据故障转移（见 [`Self::call_api_with_retry`]）
    pub async fn call_api(&self, request_body: &str) -> anyhow::Result<reqwest::Response> {
        self.call_api_with_auth(request_body, RequestAuthContext::default())
            .await
    }

    pub async fn call_api_with_auth(
        &self,
        request_body: &str,
        auth_context: RequestAuthContext,
    ) -> anyhow::Result<reqwest::Response> {
        self.call_api_with_retry(request_body, false, auth_context)
            .await
    }

    /// 发送流式 API 请求
    pub async fn call_api_stream(&self, request_body: &str) -> anyhow::Result<reqwest::Response> {
        self.call_api_stream_with_auth(request_body, RequestAuthContext::default())
            .await
    }

    pub async fn call_api_stream_with_auth(
        &self,
        request_body: &str,
        auth_context: RequestAuthContext,
    ) -> anyhow::Result<reqwest::Response> {
        self.call_api_with_retry(request_body, true, auth_context)
            .await
    }

    /// 发送 MCP API 请求（WebSearch 等工具调用）
    pub async fn call_mcp(&self, request_body: &str) -> anyhow::Result<reqwest::Response> {
        self.call_mcp_with_auth(request_body, RequestAuthContext::default())
            .await
    }

    pub async fn call_mcp_with_auth(
        &self,
        request_body: &str,
        auth_context: RequestAuthContext,
    ) -> anyhow::Result<reqwest::Response> {
        self.call_mcp_with_retry(request_body, auth_context).await
    }

    /// 内部方法：带重试逻辑的 MCP API 调用
    async fn call_mcp_with_retry(
        &self,
        request_body: &str,
        auth_context: RequestAuthContext,
    ) -> anyhow::Result<reqwest::Response> {
        // MCP 调用（WebSearch 等工具）不涉及模型选择，无需按模型过滤凭据
        let req = KiroRequest::Mcp { body: request_body };
        self.call_with_retry(&req, None, "MCP 请求", auth_context)
            .await
    }

    fn record_usage_async(
        &self,
        auth_context: &RequestAuthContext,
        ctx: &crate::kiro::token_manager::CallContext,
        req: &KiroRequest<'_>,
        status: &str,
        http_status: Option<u16>,
        error_kind: Option<&str>,
        duration_ms: i64,
    ) {
        let Some(store) = self.token_manager.account_store() else {
            return;
        };
        let model = match req {
            KiroRequest::GenerateAssistant { model, .. } => model.map(str::to_string),
            KiroRequest::Mcp { .. } => Some("mcp".to_string()),
            _ => None,
        };
        let stream = matches!(req, KiroRequest::GenerateAssistant { stream: true, .. });
        let endpoint = ctx.credentials().endpoint.clone();
        let draft = UsageLogDraft {
            api_key_id: auth_context.api_key_id,
            group_id: auth_context.group_id,
            account_id: ctx.id(),
            model,
            endpoint,
            stream,
            status: status.to_string(),
            http_status,
            error_kind: error_kind.map(str::to_string),
            input_tokens: 0,
            output_tokens: 0,
            duration_ms: Some(duration_ms),
        };
        tokio::spawn(async move {
            if let Err(err) = store.record_usage(draft).await {
                tracing::warn!("写入使用日志失败: {}", err);
            }
        });
    }

    /// 内部方法：带重试逻辑的 API 调用
    ///
    /// 重试策略：
    /// - 每个凭据最多重试 MAX_RETRIES_PER_CREDENTIAL 次
    /// - 总重试次数 = min(凭据数量 × 每凭据重试次数, MAX_TOTAL_RETRIES)
    /// - 硬上限 9 次，避免无限重试
    async fn call_api_with_retry(
        &self,
        request_body: &str,
        is_stream: bool,
        auth_context: RequestAuthContext,
    ) -> anyhow::Result<reqwest::Response> {
        let model = Self::extract_model_from_request(request_body);
        let kind_label = if is_stream {
            "流式 API 请求"
        } else {
            "非流式 API 请求"
        };
        let req = KiroRequest::GenerateAssistant {
            body: request_body,
            stream: is_stream,
            model: model.as_deref(),
        };
        self.call_with_retry(&req, model.as_deref(), kind_label, auth_context)
            .await
    }

    /// 统一的带重试循环实现
    ///
    /// # 参数
    /// - `req`: KiroRequest 变体（内部仅含引用，单次构造即可多次传给 build_request）
    /// - `model_for_acquire`: 传给 `acquire_context` 用于凭据过滤（opus 等模型）
    /// - `kind_label`: 日志与错误消息前缀（如 "MCP 请求" / "流式 API 请求" / "非流式 API 请求"）
    ///
    /// 该 helper 承载 §5 所有契约：
    /// - force_refreshed Set 语义：每凭据一次强制刷新机会
    /// - 错误分支顺序：402 → 400 → 401/403 → 瞬态 → 其他 4xx → 兜底
    /// - anyhow 错误消息格式：统一通过 `kind_label` 前缀（字节级等价于重构前
    ///   "{api_type} API 请求失败"、"MCP 请求失败"）
    /// - sleep 时机与 retry 次数
    async fn call_with_retry(
        &self,
        req: &KiroRequest<'_>,
        model_for_acquire: Option<&str>,
        kind_label: &str,
        auth_context: RequestAuthContext,
    ) -> anyhow::Result<reqwest::Response> {
        let total_credentials = self.token_manager.total_count();
        let max_retries = (total_credentials * MAX_RETRIES_PER_CREDENTIAL).min(MAX_TOTAL_RETRIES);
        let mut last_error: Option<anyhow::Error> = None;
        let mut force_refreshed: HashSet<u64> = HashSet::new();

        for attempt in 0..max_retries {
            let ctx = match self
                .token_manager
                .acquire_context(model_for_acquire, auth_context.group_id)
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    last_error = Some(e);
                    continue;
                }
            };

            let config = self.token_manager.config();
            let endpoint = ctx.endpoint();
            let rctx = ctx.to_request_context(config);

            let client = self.client_for(ctx.credentials())?;
            let request = endpoint.build_request(&client, &rctx, req)?;
            let started_at = Instant::now();

            let response = match request.send().await {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::warn!(
                        "{}发送失败（尝试 {}/{}）: {}",
                        kind_label,
                        attempt + 1,
                        max_retries,
                        e
                    );
                    last_error = Some(e.into());
                    if attempt + 1 < max_retries {
                        sleep(Self::retry_delay(attempt)).await;
                    }
                    continue;
                }
            };

            let status = response.status();

            // 成功响应
            if status.is_success() {
                self.token_manager.report_success(ctx.id());
                self.record_usage_async(
                    &auth_context,
                    &ctx,
                    req,
                    "success",
                    Some(status.as_u16()),
                    None,
                    started_at.elapsed().as_millis() as i64,
                );
                return Ok(response);
            }

            // 失败响应：读取 body 用于日志/错误信息
            let body = match response.text().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("读取响应 body 失败: {e}");
                    String::new()
                }
            };

            let status_code = status.as_u16();
            if let Some(rule) =
                Self::match_temp_unschedulable_rule(ctx.credentials(), status_code, &body)
            {
                let duration_minutes = rule.duration_minutes.max(1);
                let until = chrono::Utc::now() + chrono::Duration::minutes(duration_minutes);
                let reason = rule
                    .description
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("临时冷却规则命中");
                self.token_manager
                    .set_temp_unschedulable(ctx.id(), until, reason.to_string());
                tracing::warn!(
                    "{}失败（命中临时冷却规则，账号 #{} 冷却 {} 分钟，尝试 {}/{}）: {} {}",
                    kind_label,
                    ctx.id(),
                    duration_minutes,
                    attempt + 1,
                    max_retries,
                    status_code,
                    body
                );
                last_error = Some(anyhow::anyhow!(
                    "{}失败（账号临时冷却）: {} {}",
                    kind_label,
                    status_code,
                    body
                ));
                continue;
            }

            let error_kind = endpoint.classify_error(status_code, &body);
            self.record_usage_async(
                &auth_context,
                &ctx,
                req,
                "error",
                Some(status.as_u16()),
                Some(match error_kind {
                    EndpointErrorKind::MonthlyQuotaExhausted => "monthly_quota_exhausted",
                    EndpointErrorKind::BadRequest => "bad_request",
                    EndpointErrorKind::BearerTokenInvalid => "bearer_token_invalid",
                    EndpointErrorKind::Unauthorized => "unauthorized",
                    EndpointErrorKind::RateLimited => "rate_limited",
                    EndpointErrorKind::Overloaded => "overloaded",
                    EndpointErrorKind::Transient => "transient",
                    EndpointErrorKind::ClientError => "client_error",
                    EndpointErrorKind::Unknown => "unknown",
                }),
                started_at.elapsed().as_millis() as i64,
            );

            match error_kind {
                EndpointErrorKind::MonthlyQuotaExhausted => {
                    tracing::warn!(
                        "{}失败（额度已用尽，禁用凭据并切换，尝试 {}/{}）: {} {}",
                        kind_label,
                        attempt + 1,
                        max_retries,
                        status,
                        body
                    );

                    let has_available = self.token_manager.report_quota_exhausted(ctx.id());
                    if !has_available {
                        anyhow::bail!("{}失败（所有凭据已用尽）: {} {}", kind_label, status, body);
                    }

                    last_error = Some(anyhow::anyhow!("{}失败: {} {}", kind_label, status, body));
                    continue;
                }
                EndpointErrorKind::BadRequest => {
                    anyhow::bail!("{}失败: {} {}", kind_label, status, body);
                }
                EndpointErrorKind::BearerTokenInvalid => {
                    tracing::warn!(
                        "{}失败（可能为凭据错误，尝试 {}/{}）: {} {}",
                        kind_label,
                        attempt + 1,
                        max_retries,
                        status,
                        body
                    );

                    // token 被上游失效：先尝试 force-refresh，每凭据仅一次机会
                    let id = ctx.id();
                    if !force_refreshed.contains(&id) {
                        force_refreshed.insert(id);
                        tracing::info!("凭据 #{} token 疑似被上游失效，尝试强制刷新", id);
                        if self.token_manager.force_refresh_token_for(id).await.is_ok() {
                            tracing::info!("凭据 #{} token 强制刷新成功，重试请求", id);
                            continue;
                        }
                        tracing::warn!("凭据 #{} token 强制刷新失败，计入失败", id);
                    }

                    let has_available = self.token_manager.report_failure(ctx.id());
                    if !has_available {
                        anyhow::bail!("{}失败（所有凭据已用尽）: {} {}", kind_label, status, body);
                    }

                    last_error = Some(anyhow::anyhow!("{}失败: {} {}", kind_label, status, body));
                    continue;
                }
                EndpointErrorKind::Unauthorized => {
                    // 注：provider 层暂无 mock/集成测试基础设施，此 arm 的端到端
                    // 行为依赖 endpoint 层 classify_error 测试 + 人工走查覆盖
                    // （见 docs/plans/2026-04-23-401-403-failover-regression-plan.md Phase 4）
                    tracing::warn!(
                        "{}失败（可能为凭据错误，尝试 {}/{}）: {} {}",
                        kind_label,
                        attempt + 1,
                        max_retries,
                        status,
                        body
                    );

                    let has_available = self.token_manager.report_failure(ctx.id());
                    if !has_available {
                        anyhow::bail!("{}失败（所有凭据已用尽）: {} {}", kind_label, status, body);
                    }

                    last_error = Some(anyhow::anyhow!("{}失败: {} {}", kind_label, status, body));
                    continue;
                }
                EndpointErrorKind::RateLimited => {
                    // 429 速率限制：设置冷却时间并切换账号重试
                    tracing::warn!(
                        "{}失败（速率限制，尝试 {}/{}）: {} {}",
                        kind_label,
                        attempt + 1,
                        max_retries,
                        status,
                        body
                    );

                    // TODO: 解析 Retry-After header 或响应体中的重置时间
                    // 当前默认冷却 60 秒
                    let reset_at = chrono::Utc::now() + chrono::Duration::seconds(60);
                    self.token_manager.set_rate_limit(ctx.id(), reset_at);

                    // 立即切换到下一个可用账号，不消耗重试次数
                    last_error = Some(anyhow::anyhow!(
                        "{}失败（速率限制）: {} {}",
                        kind_label,
                        status,
                        body
                    ));
                    continue;
                }
                EndpointErrorKind::Overloaded => {
                    // 529 服务端过载：设置冷却时间并切换账号重试
                    tracing::warn!(
                        "{}失败（服务端过载，尝试 {}/{}）: {} {}",
                        kind_label,
                        attempt + 1,
                        max_retries,
                        status,
                        body
                    );

                    // 默认冷却 10 分钟
                    let overload_until = chrono::Utc::now() + chrono::Duration::minutes(10);
                    self.token_manager.set_overload(ctx.id(), overload_until);

                    // 立即切换到下一个可用账号，不消耗重试次数
                    last_error = Some(anyhow::anyhow!(
                        "{}失败（过载）: {} {}",
                        kind_label,
                        status,
                        body
                    ));
                    continue;
                }
                EndpointErrorKind::Transient => {
                    tracing::warn!(
                        "{}失败（上游瞬态错误，尝试 {}/{}）: {} {}",
                        kind_label,
                        attempt + 1,
                        max_retries,
                        status,
                        body
                    );
                    last_error = Some(anyhow::anyhow!("{}失败: {} {}", kind_label, status, body));
                    if attempt + 1 < max_retries {
                        sleep(Self::retry_delay(attempt)).await;
                    }
                    continue;
                }
                EndpointErrorKind::ClientError => {
                    anyhow::bail!("{}失败: {} {}", kind_label, status, body);
                }
                EndpointErrorKind::Unknown => {
                    // 兜底：当作可重试的瞬态错误处理（不切换凭据）
                    tracing::warn!(
                        "{}失败（未知错误，尝试 {}/{}）: {} {}",
                        kind_label,
                        attempt + 1,
                        max_retries,
                        status,
                        body
                    );
                    last_error = Some(anyhow::anyhow!("{}失败: {} {}", kind_label, status, body));
                    if attempt + 1 < max_retries {
                        sleep(Self::retry_delay(attempt)).await;
                    }
                    continue;
                }
            }
        }

        // 所有重试都失败
        Err(last_error.unwrap_or_else(|| {
            anyhow::anyhow!(
                "{}失败：已达到最大重试次数（{}次）",
                kind_label,
                max_retries
            )
        }))
    }

    /// 从请求体中提取模型信息
    ///
    /// 尝试解析 JSON 请求体，提取 conversationState.currentMessage.userInputMessage.modelId
    fn extract_model_from_request(request_body: &str) -> Option<String> {
        use serde_json::Value;

        let json: Value = serde_json::from_str(request_body).ok()?;

        json.get("conversationState")?
            .get("currentMessage")?
            .get("userInputMessage")?
            .get("modelId")?
            .as_str()
            .map(|s| s.to_string())
    }

    fn retry_delay(attempt: usize) -> Duration {
        // 指数退避 + 少量抖动，避免上游抖动时放大故障
        const BASE_MS: u64 = 200;
        const MAX_MS: u64 = 2_000;
        let exp = BASE_MS.saturating_mul(2u64.saturating_pow(attempt.min(6) as u32));
        let backoff = exp.min(MAX_MS);
        let jitter_max = (backoff / 4).max(1);
        let jitter = fastrand::u64(0..=jitter_max);
        Duration::from_millis(backoff.saturating_add(jitter))
    }

    fn match_temp_unschedulable_rule<'a>(
        credentials: &'a KiroCredentials,
        status_code: u16,
        body: &str,
    ) -> Option<&'a TempUnschedulableRule> {
        if !credentials.temp_unschedulable_enabled {
            return None;
        }

        let body_lower = body.to_lowercase();
        credentials.temp_unschedulable_rules.iter().find(|rule| {
            rule.error_code == status_code
                && rule.duration_minutes > 0
                && !rule.keywords.is_empty()
                && rule
                    .keywords
                    .iter()
                    .map(|keyword| keyword.trim().to_lowercase())
                    .any(|keyword| !keyword.is_empty() && body_lower.contains(&keyword))
        })
    }
}
