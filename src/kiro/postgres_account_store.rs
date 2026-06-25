use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

use crate::admin::types::BalanceResponse;
use crate::kiro::account_store::{
    AccountRecord, ApiKeyAuthRecord, ApiKeyRecord, CreateApiKeyRequest, CreateApiKeyResponse,
    CreateGroupRequest, GroupRecord, GroupSummary, UpdateApiKeyRequest, UpdateGroupRequest,
    UsageLogDraft, UsageLogItem, UsageLogsQuery, UsageLogsResponse,
};
use crate::kiro::model::credentials::KiroCredentials;

#[derive(Clone)]
pub struct PostgresAccountStore {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatsEntry {
    success_count: u64,
    last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedBalance {
    cached_at: f64,
    data: BalanceResponse,
}

impl PostgresAccountStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn import_from_json_if_needed(
        &self,
        credentials_path: &Path,
        credentials: &[KiroCredentials],
    ) -> anyhow::Result<()> {
        let imported: Option<String> =
            sqlx::query_scalar("SELECT value FROM schema_meta WHERE key = 'json_imported'")
                .fetch_optional(&self.pool)
                .await?;
        if imported.is_some() {
            return Ok(());
        }

        let account_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE deleted_at IS NULL")
                .fetch_one(&self.pool)
                .await?;
        if account_count > 0 {
            self.set_meta("json_imported", "skipped_existing_accounts")
                .await?;
            return Ok(());
        }

        let default_group_id = self.ensure_default_group().await?;
        let stats = self.load_stats_json(credentials_path);

        for credential in credentials {
            let id = credential.id.unwrap_or(0);
            let stat = id
                .checked_sub(0)
                .and_then(|_| stats.get(&id.to_string()).cloned());
            let account_id = self
                .insert_account_with_optional_id(credential, id, stat.as_ref())
                .await?;
            self.set_account_groups(account_id, &[default_group_id])
                .await?;
        }

        self.import_balance_cache(credentials_path).await?;
        self.set_meta("json_imported", "true").await?;
        tracing::info!("已将 {} 个 JSON 凭据导入 PostgreSQL", credentials.len());
        Ok(())
    }

    pub async fn load_accounts(&self) -> anyhow::Result<Vec<AccountRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, credential_json, failure_count, refresh_failure_count, success_count,
                   last_used_at, status, schedulable, disabled_reason,
                   temp_unschedulable_until, temp_unschedulable_reason,
                   rate_limit_reset_at, overload_until
            FROM accounts
            WHERE deleted_at IS NULL
            ORDER BY priority ASC, id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut accounts = Vec::with_capacity(rows.len());
        for row in rows {
            let id: i64 = row.get("id");
            let mut credentials: KiroCredentials =
                serde_json::from_str(row.get("credential_json"))?;
            credentials.id = Some(id as u64);
            let status: String = row.get("status");
            let schedulable: bool = row.get("schedulable");
            let disabled = status != "active" || !schedulable || credentials.disabled;
            let groups = self.get_account_groups(id as u64).await?;
            accounts.push(AccountRecord {
                id: id as u64,
                credentials,
                failure_count: row.get::<i64, _>("failure_count") as u32,
                refresh_failure_count: row.get::<i64, _>("refresh_failure_count") as u32,
                disabled,
                disabled_reason: row.get("disabled_reason"),
                success_count: row.get::<i64, _>("success_count") as u64,
                last_used_at: row.get("last_used_at"),
                schedulable,
                status,
                temp_unschedulable_until: row.get("temp_unschedulable_until"),
                temp_unschedulable_reason: row.get("temp_unschedulable_reason"),
                rate_limit_reset_at: row.get("rate_limit_reset_at"),
                overload_until: row.get("overload_until"),
                groups,
            });
        }
        Ok(accounts)
    }

    pub async fn persist_account(&self, record: &AccountRecord) -> anyhow::Result<()> {
        let credential_json = serde_json::to_string(&record.credentials)?;
        let supported_models = serde_json::to_string(&record.credentials.supported_models)?;
        let auth_method = if record.credentials.is_api_key_credential() {
            "api_key".to_string()
        } else {
            record
                .credentials
                .auth_method
                .clone()
                .unwrap_or_else(|| "social".to_string())
        };
        let status = if record.disabled {
            "disabled"
        } else {
            "active"
        };
        let schedulable = !record.disabled;

        sqlx::query(
            r#"
            UPDATE accounts
            SET credential_json = $1, auth_method = $2, email = $3, credential_hash = $4, priority = $5,
                status = $6, schedulable = $7, disabled_reason = $8, failure_count = $9,
                refresh_failure_count = $10, success_count = $11, last_used_at = $12, expires_at = $13,
                subscription_title = $14, endpoint = $15, supported_models = $16,
                temp_unschedulable_until = $17, temp_unschedulable_reason = $18,
                rate_limit_reset_at = $19, overload_until = $20, updated_at = CURRENT_TIMESTAMP
            WHERE id = $21 AND deleted_at IS NULL
            "#,
        )
        .bind(credential_json)
        .bind(auth_method)
        .bind(&record.credentials.email)
        .bind(credential_hash(&record.credentials))
        .bind(record.credentials.priority as i64)
        .bind(status)
        .bind(schedulable)
        .bind(&record.disabled_reason)
        .bind(record.failure_count as i64)
        .bind(record.refresh_failure_count as i64)
        .bind(record.success_count as i64)
        .bind(&record.last_used_at)
        .bind(&record.credentials.expires_at)
        .bind(&record.credentials.subscription_title)
        .bind(&record.credentials.endpoint)
        .bind(supported_models)
        .bind(&record.temp_unschedulable_until)
        .bind(&record.temp_unschedulable_reason)
        .bind(&record.rate_limit_reset_at)
        .bind(&record.overload_until)
        .bind(record.id as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_account(
        &self,
        credential: &KiroCredentials,
        group_ids: &[i64],
    ) -> anyhow::Result<u64> {
        let account_id = self
            .insert_account_with_optional_id(credential, 0, None)
            .await?;
        let groups = if group_ids.is_empty() {
            vec![self.ensure_default_group().await?]
        } else {
            group_ids.to_vec()
        };
        self.set_account_groups(account_id, &groups).await?;
        Ok(account_id)
    }

    pub async fn soft_delete_account(&self, id: u64) -> anyhow::Result<()> {
        sqlx::query("UPDATE accounts SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn record_usage(&self, draft: UsageLogDraft) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO usage_logs (
                api_key_id, group_id, account_id, model, endpoint, stream, status,
                http_status, error_kind, input_tokens, output_tokens, duration_ms
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(draft.api_key_id)
        .bind(draft.group_id)
        .bind(draft.account_id as i64)
        .bind(draft.model)
        .bind(draft.endpoint)
        .bind(draft.stream)
        .bind(draft.status)
        .bind(draft.http_status.map(i64::from))
        .bind(draft.error_kind)
        .bind(draft.input_tokens)
        .bind(draft.output_tokens)
        .bind(draft.duration_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn default_group_id(&self) -> anyhow::Result<i64> {
        self.ensure_default_group().await
    }

    pub async fn find_api_key(&self, key: &str) -> anyhow::Result<Option<ApiKeyAuthRecord>> {
        let hash = sha256_hex(key);
        let row = sqlx::query(
            r#"
            SELECT id, group_id
            FROM api_keys
            WHERE key_hash = $1
              AND status = 'active'
              AND deleted_at IS NULL
              AND (expires_at IS NULL OR expires_at > to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'))
            "#,
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let id: i64 = row.get("id");
        sqlx::query("UPDATE api_keys SET last_used_at = $1, updated_at = $1 WHERE id = $2")
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(Some(ApiKeyAuthRecord {
            id,
            group_id: row.get("group_id"),
        }))
    }

    pub async fn list_api_keys(&self) -> anyhow::Result<Vec<ApiKeyRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT ak.id, ak.name, ak.status, ak.group_id, g.name AS group_name,
                   ak.last_used_at, ak.quota_limit, ak.quota_used, ak.expires_at, ak.created_at
            FROM api_keys ak
            LEFT JOIN groups g ON g.id = ak.group_id
            WHERE ak.deleted_at IS NULL
            ORDER BY ak.id DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(api_key_record_from_row).collect())
    }

    pub async fn create_api_key(
        &self,
        req: CreateApiKeyRequest,
    ) -> anyhow::Result<CreateApiKeyResponse> {
        let name = req.name.trim();
        if name.is_empty() {
            anyhow::bail!("API Key 名称不能为空");
        }
        if req.quota_limit < 0.0 {
            anyhow::bail!("quotaLimit 不能小于 0");
        }
        if let Some(group_id) = req.group_id {
            self.get_group(group_id).await?;
        }

        let plain_key = format!("sk-kiro-{}", random_key(32));
        let key_hash = sha256_hex(&plain_key);
        let id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO api_keys (key_hash, name, group_id, quota_limit, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(key_hash)
        .bind(name)
        .bind(req.group_id)
        .bind(req.quota_limit)
        .bind(req.expires_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(CreateApiKeyResponse {
            success: true,
            message: "API Key 已创建，请立即保存明文 key".to_string(),
            api_key: plain_key,
            record: self.get_api_key(id).await?,
        })
    }

    pub async fn update_api_key(
        &self,
        id: i64,
        req: UpdateApiKeyRequest,
    ) -> anyhow::Result<ApiKeyRecord> {
        let current = self.get_api_key(id).await?;
        let name = req.name.unwrap_or(current.name).trim().to_string();
        if name.is_empty() {
            anyhow::bail!("API Key 名称不能为空");
        }
        let status = req.status.unwrap_or(current.status);
        if status != "active" && status != "disabled" {
            anyhow::bail!("API Key 状态必须是 active 或 disabled");
        }
        if let Some(group_id) = req.group_id {
            self.get_group(group_id).await?;
        }
        let quota_limit = req.quota_limit.unwrap_or(current.quota_limit);
        if quota_limit < 0.0 {
            anyhow::bail!("quotaLimit 不能小于 0");
        }
        sqlx::query(
            r#"
            UPDATE api_keys
            SET name = $1, status = $2, group_id = $3, quota_limit = $4, expires_at = $5,
                updated_at = $6
            WHERE id = $7 AND deleted_at IS NULL
            "#,
        )
        .bind(name)
        .bind(status)
        .bind(req.group_id.or(current.group_id))
        .bind(quota_limit)
        .bind(req.expires_at.or(current.expires_at))
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.get_api_key(id).await
    }

    pub async fn delete_api_key(&self, id: i64) -> anyhow::Result<()> {
        let result = sqlx::query(
            "UPDATE api_keys SET deleted_at = $1, updated_at = $1 WHERE id = $2 AND deleted_at IS NULL",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("API Key 不存在: {}", id);
        }
        Ok(())
    }

    pub async fn rotate_api_key(&self, id: i64) -> anyhow::Result<CreateApiKeyResponse> {
        let current = self.get_api_key(id).await?;
        let plain_key = format!("sk-kiro-{}", random_key(32));
        sqlx::query("UPDATE api_keys SET key_hash = $1, updated_at = $2 WHERE id = $3")
            .bind(sha256_hex(&plain_key))
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(CreateApiKeyResponse {
            success: true,
            message: "API Key 已轮换，请立即保存新明文 key".to_string(),
            api_key: plain_key,
            record: self.get_api_key(current.id).await?,
        })
    }

    pub async fn list_groups(&self) -> anyhow::Result<Vec<GroupRecord>> {
        let rows = sqlx::query(
            "SELECT id, name, description, status, priority, is_default FROM groups WHERE deleted_at IS NULL ORDER BY is_default DESC, priority ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| GroupRecord {
                id: row.get("id"),
                name: row.get("name"),
                description: row.get("description"),
                status: row.get("status"),
                priority: row.get("priority"),
                is_default: row.get("is_default"),
            })
            .collect())
    }

    pub async fn create_group(&self, req: CreateGroupRequest) -> anyhow::Result<GroupRecord> {
        let name = req.name.trim();
        if name.is_empty() {
            anyhow::bail!("分组名称不能为空");
        }
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO groups (name, description, priority) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(name)
        .bind(
            req.description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        )
        .bind(req.priority)
        .fetch_one(&self.pool)
        .await?;
        self.get_group(id).await
    }

    pub async fn update_group(
        &self,
        id: i64,
        req: UpdateGroupRequest,
    ) -> anyhow::Result<GroupRecord> {
        let current = self.get_group(id).await?;
        let name = req.name.unwrap_or(current.name).trim().to_string();
        if name.is_empty() {
            anyhow::bail!("分组名称不能为空");
        }
        let status = req.status.unwrap_or(current.status);
        if status != "active" && status != "disabled" {
            anyhow::bail!("分组状态必须是 active 或 disabled");
        }
        sqlx::query(
            "UPDATE groups SET name = $1, description = $2, status = $3, priority = $4, updated_at = CURRENT_TIMESTAMP WHERE id = $5 AND deleted_at IS NULL",
        )
        .bind(name)
        .bind(req.description.or(current.description))
        .bind(status)
        .bind(req.priority.unwrap_or(current.priority))
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.get_group(id).await
    }

    pub async fn delete_group(&self, id: i64) -> anyhow::Result<()> {
        let group = self.get_group(id).await?;
        if group.is_default {
            anyhow::bail!("默认分组不能删除");
        }
        sqlx::query("UPDATE groups SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_account_groups(&self, account_id: u64) -> anyhow::Result<Vec<GroupSummary>> {
        let rows = sqlx::query(
            r#"
            SELECT g.id, g.name
            FROM groups g
            JOIN account_groups ag ON ag.group_id = g.id
            WHERE ag.account_id = $1 AND g.deleted_at IS NULL
            ORDER BY ag.priority ASC, g.priority ASC, g.id ASC
            "#,
        )
        .bind(account_id as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| GroupSummary {
                id: row.get("id"),
                name: row.get("name"),
            })
            .collect())
    }

    pub async fn set_account_groups(
        &self,
        account_id: u64,
        group_ids: &[i64],
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM account_groups WHERE account_id = $1")
            .bind(account_id as i64)
            .execute(&mut *tx)
            .await?;

        let group_ids = if group_ids.is_empty() {
            vec![self.ensure_default_group_in_tx(&mut tx).await?]
        } else {
            let mut seen = HashSet::new();
            group_ids
                .iter()
                .copied()
                .filter(|id| seen.insert(*id))
                .collect()
        };

        for group_id in group_ids {
            sqlx::query("INSERT INTO account_groups (account_id, group_id) VALUES ($1, $2)")
                .bind(account_id as i64)
                .bind(group_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_usage_logs(
        &self,
        query: UsageLogsQuery,
    ) -> anyhow::Result<UsageLogsResponse> {
        let page_size = match query.page_size {
            50 | 100 | 200 => query.page_size,
            0 => 50,
            other => anyhow::bail!("pageSize 必须是 50、100 或 200（当前: {}）", other),
        };
        let page = query.page.max(1);
        let offset = ((page - 1) * page_size) as i64;
        let limit = page_size as i64;

        let mut where_sql = String::from("WHERE 1 = 1");
        let mut bind_values = Vec::new();
        if let Some(id) = query.account_id {
            bind_values.push(id as i64);
            where_sql.push_str(&format!(" AND account_id = ${}", bind_values.len()));
        }
        if let Some(id) = query.group_id {
            bind_values.push(id);
            where_sql.push_str(&format!(" AND group_id = ${}", bind_values.len()));
        }

        let count_sql = format!("SELECT COUNT(*) FROM usage_logs {where_sql}");
        let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
        for value in &bind_values {
            count_query = count_query.bind(*value);
        }
        let total_items = count_query.fetch_one(&self.pool).await? as usize;

        let limit_param = bind_values.len() + 1;
        let offset_param = bind_values.len() + 2;
        let list_sql = format!(
            r#"
            SELECT ul.id, ul.api_key_id, ul.group_id, g.name AS group_name, ul.account_id,
                   ul.model, ul.endpoint, ul.stream, ul.status, ul.http_status, ul.error_kind,
                   ul.input_tokens, ul.output_tokens, ul.duration_ms, ul.created_at
            FROM usage_logs ul
            LEFT JOIN groups g ON g.id = ul.group_id
            {where_sql}
            ORDER BY ul.id DESC
            LIMIT ${limit_param} OFFSET ${offset_param}
            "#
        );
        let mut list_query = sqlx::query(&list_sql);
        for value in &bind_values {
            list_query = list_query.bind(*value);
        }
        let rows = list_query
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        let logs = rows
            .into_iter()
            .map(|row| UsageLogItem {
                id: row.get::<i64, _>("id"),
                api_key_id: row.get("api_key_id"),
                group_id: row.get("group_id"),
                group_name: row.get("group_name"),
                account_id: row.get::<i64, _>("account_id") as u64,
                model: row.get("model"),
                endpoint: row.get("endpoint"),
                stream: row.get("stream"),
                status: row.get("status"),
                http_status: row.get::<Option<i64>, _>("http_status").map(|v| v as u16),
                error_kind: row.get("error_kind"),
                input_tokens: row.get("input_tokens"),
                output_tokens: row.get("output_tokens"),
                duration_ms: row.get("duration_ms"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(UsageLogsResponse {
            logs,
            page,
            page_size,
            total_items,
            total_pages: total_items.div_ceil(page_size).max(1),
        })
    }

    pub async fn get_balance_cache(
        &self,
        account_id: u64,
    ) -> anyhow::Result<Option<BalanceResponse>> {
        let row =
            sqlx::query("SELECT cached_at, data_json FROM balance_cache WHERE account_id = $1")
                .bind(account_id as i64)
                .fetch_optional(&self.pool)
                .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let cached_at: String = row.get("cached_at");
        let cached_at = chrono::DateTime::parse_from_rfc3339(&cached_at)
            .ok()
            .map(|dt| dt.timestamp())
            .unwrap_or(0);
        if Utc::now().timestamp() - cached_at >= 300 {
            return Ok(None);
        }
        let data: BalanceResponse = serde_json::from_str(row.get("data_json"))?;
        Ok(Some(data))
    }

    pub async fn set_balance_cache(
        &self,
        account_id: u64,
        data: &BalanceResponse,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO balance_cache (account_id, cached_at, data_json)
            VALUES ($1, $2, $3)
            ON CONFLICT(account_id) DO UPDATE SET cached_at = excluded.cached_at, data_json = excluded.data_json
            "#,
        )
        .bind(account_id as i64)
        .bind(Utc::now().to_rfc3339())
        .bind(serde_json::to_string(data)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_balance_cache(&self, account_id: u64) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM balance_cache WHERE account_id = $1")
            .bind(account_id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn insert_account_with_optional_id(
        &self,
        credential: &KiroCredentials,
        preferred_id: u64,
        stat: Option<&StatsEntry>,
    ) -> anyhow::Result<u64> {
        let credential_json = serde_json::to_string(credential)?;
        let supported_models = serde_json::to_string(&credential.supported_models)?;
        let auth_method = if credential.is_api_key_credential() {
            "api_key".to_string()
        } else {
            credential
                .auth_method
                .clone()
                .unwrap_or_else(|| "social".to_string())
        };
        let status = if credential.disabled {
            "disabled"
        } else {
            "active"
        };
        let schedulable = !credential.disabled;
        let success_count = stat.map(|s| s.success_count).unwrap_or(0) as i64;
        let last_used_at = stat.and_then(|s| s.last_used_at.clone());

        if preferred_id > 0 {
            sqlx::query(
                r#"
                INSERT INTO accounts (
                    id, email, auth_method, credential_json, credential_hash, priority, status,
                    schedulable, disabled_reason, success_count, last_used_at, expires_at,
                    subscription_title, endpoint, supported_models
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                "#,
            )
            .bind(preferred_id as i64)
            .bind(&credential.email)
            .bind(&auth_method)
            .bind(&credential_json)
            .bind(credential_hash(credential))
            .bind(credential.priority as i64)
            .bind(status)
            .bind(schedulable)
            .bind(if credential.disabled {
                Some("Manual")
            } else {
                None
            })
            .bind(success_count)
            .bind(&last_used_at)
            .bind(&credential.expires_at)
            .bind(&credential.subscription_title)
            .bind(&credential.endpoint)
            .bind(&supported_models)
            .execute(&self.pool)
            .await?;
            Ok(preferred_id)
        } else {
            let id: i64 = sqlx::query_scalar(
                r#"
                INSERT INTO accounts (
                    email, auth_method, credential_json, credential_hash, priority, status,
                    schedulable, disabled_reason, success_count, last_used_at, expires_at,
                    subscription_title, endpoint, supported_models
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                RETURNING id
                "#,
            )
            .bind(&credential.email)
            .bind(&auth_method)
            .bind(&credential_json)
            .bind(credential_hash(credential))
            .bind(credential.priority as i64)
            .bind(status)
            .bind(schedulable)
            .bind(if credential.disabled {
                Some("Manual")
            } else {
                None
            })
            .bind(success_count)
            .bind(&last_used_at)
            .bind(&credential.expires_at)
            .bind(&credential.subscription_title)
            .bind(&credential.endpoint)
            .bind(&supported_models)
            .fetch_one(&self.pool)
            .await?;
            Ok(id as u64)
        }
    }

    async fn get_api_key(&self, id: i64) -> anyhow::Result<ApiKeyRecord> {
        let row = sqlx::query(
            r#"
            SELECT ak.id, ak.name, ak.status, ak.group_id, g.name AS group_name,
                   ak.last_used_at, ak.quota_limit, ak.quota_used, ak.expires_at, ak.created_at
            FROM api_keys ak
            LEFT JOIN groups g ON g.id = ak.group_id
            WHERE ak.id = $1 AND ak.deleted_at IS NULL
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("API Key 不存在: {}", id))?;
        Ok(api_key_record_from_row(row))
    }

    async fn get_group(&self, id: i64) -> anyhow::Result<GroupRecord> {
        let row = sqlx::query(
            "SELECT id, name, description, status, priority, is_default FROM groups WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("分组不存在: {}", id))?;
        Ok(GroupRecord {
            id: row.get("id"),
            name: row.get("name"),
            description: row.get("description"),
            status: row.get("status"),
            priority: row.get("priority"),
            is_default: row.get("is_default"),
        })
    }

    async fn ensure_default_group(&self) -> anyhow::Result<i64> {
        let mut tx = self.pool.begin().await?;
        let id = self.ensure_default_group_in_tx(&mut tx).await?;
        tx.commit().await?;
        Ok(id)
    }

    async fn ensure_default_group_in_tx<'a>(
        &self,
        tx: &mut sqlx::Transaction<'a, sqlx::Postgres>,
    ) -> anyhow::Result<i64> {
        if let Some(id) = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM groups WHERE is_default = TRUE AND deleted_at IS NULL LIMIT 1",
        )
        .fetch_optional(&mut **tx)
        .await?
        {
            return Ok(id);
        }
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO groups (name, description, status, priority, is_default) VALUES ('default', '默认账号组', 'active', 0, TRUE) RETURNING id",
        )
        .fetch_one(&mut **tx)
        .await?;
        Ok(id)
    }

    async fn set_meta(&self, key: &str, value: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO schema_meta (key, value, updated_at) VALUES ($1, $2, CURRENT_TIMESTAMP) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn load_stats_json(&self, credentials_path: &Path) -> HashMap<String, StatsEntry> {
        let Some(path) = credentials_path
            .parent()
            .map(|dir| dir.join("kiro_stats.json"))
        else {
            return HashMap::new();
        };
        let Ok(content) = std::fs::read_to_string(path) else {
            return HashMap::new();
        };
        serde_json::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!("解析统计缓存失败，将跳过迁移: {}", e);
            HashMap::new()
        })
    }

    async fn import_balance_cache(&self, credentials_path: &Path) -> anyhow::Result<()> {
        let Some(path) = credentials_path
            .parent()
            .map(|dir| dir.join("kiro_balance_cache.json"))
        else {
            return Ok(());
        };
        let Ok(content) = std::fs::read_to_string(path) else {
            return Ok(());
        };
        let map: HashMap<String, CachedBalance> = match serde_json::from_str(&content) {
            Ok(map) => map,
            Err(e) => {
                tracing::warn!("解析余额缓存失败，将跳过迁移: {}", e);
                return Ok(());
            }
        };
        for (id, cached) in map {
            let Ok(account_id) = id.parse::<i64>() else {
                continue;
            };
            let cached_at = chrono::DateTime::from_timestamp(cached.cached_at as i64, 0)
                .unwrap_or_else(Utc::now)
                .to_rfc3339();
            sqlx::query(
                "INSERT INTO balance_cache (account_id, cached_at, data_json) VALUES ($1, $2, $3) ON CONFLICT(account_id) DO UPDATE SET cached_at = excluded.cached_at, data_json = excluded.data_json",
            )
            .bind(account_id)
            .bind(cached_at)
            .bind(serde_json::to_string(&cached.data).context("序列化余额缓存失败")?)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
}

fn api_key_record_from_row(row: sqlx::postgres::PgRow) -> ApiKeyRecord {
    ApiKeyRecord {
        id: row.get("id"),
        name: row.get("name"),
        status: row.get("status"),
        group_id: row.get("group_id"),
        group_name: row.get("group_name"),
        last_used_at: row.get("last_used_at"),
        quota_limit: row.get("quota_limit"),
        quota_used: row.get("quota_used"),
        expires_at: row.get("expires_at"),
        created_at: row.get("created_at"),
    }
}

fn random_key(len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..len)
        .map(|_| CHARS[fastrand::usize(..CHARS.len())] as char)
        .collect()
}

fn credential_hash(credential: &KiroCredentials) -> Option<String> {
    if let Some(value) = credential.kiro_api_key.as_deref() {
        return Some(sha256_hex(value));
    }
    credential.refresh_token.as_deref().map(sha256_hex)
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}
