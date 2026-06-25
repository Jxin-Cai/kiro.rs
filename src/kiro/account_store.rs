use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::kiro::model::credentials::KiroCredentials;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct AccountRecord {
    pub id: u64,
    pub credentials: KiroCredentials,
    pub failure_count: u32,
    pub refresh_failure_count: u32,
    pub disabled: bool,
    pub disabled_reason: Option<String>,
    pub success_count: u64,
    pub last_used_at: Option<String>,
    pub schedulable: bool,
    pub status: String,
    pub temp_unschedulable_until: Option<String>,
    pub temp_unschedulable_reason: Option<String>,
    pub rate_limit_reset_at: Option<String>,
    pub overload_until: Option<String>,
    pub groups: Vec<GroupSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRecord {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: i64,
    pub is_default: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGroupRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAccountGroupsRequest {
    pub group_ids: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct ApiKeyAuthRecord {
    pub id: i64,
    pub group_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRecord {
    pub id: i64,
    pub name: String,
    pub status: String,
    pub group_id: Option<i64>,
    pub group_name: Option<String>,
    pub last_used_at: Option<String>,
    pub quota_limit: f64,
    pub quota_used: f64,
    pub expires_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub group_id: Option<i64>,
    #[serde(default)]
    pub quota_limit: f64,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApiKeyRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub group_id: Option<i64>,
    #[serde(default)]
    pub quota_limit: Option<f64>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyResponse {
    pub success: bool,
    pub message: String,
    pub api_key: String,
    pub record: ApiKeyRecord,
}

impl From<KiroCredentials> for AccountRecord {
    fn from(credentials: KiroCredentials) -> Self {
        let id = credentials.id.unwrap_or(0);
        let disabled = credentials.disabled;
        Self {
            id,
            credentials,
            failure_count: 0,
            refresh_failure_count: 0,
            disabled,
            disabled_reason: disabled.then(|| "Manual".to_string()),
            success_count: 0,
            last_used_at: None,
            schedulable: !disabled,
            status: if disabled { "disabled" } else { "active" }.to_string(),
            temp_unschedulable_until: None,
            temp_unschedulable_reason: None,
            rate_limit_reset_at: None,
            overload_until: None,
            groups: vec![GroupSummary {
                id: 1,
                name: "default".to_string(),
            }],
        }
    }
}

impl AccountRecord {
    pub fn is_schedulable(&self, model: Option<&str>) -> bool {
        if self.disabled || !self.schedulable || self.status != "active" {
            return false;
        }
        if self.temp_unschedulable_active() {
            return false;
        }
        if self.rate_limit_active() {
            return false;
        }
        if self.overload_active() {
            return false;
        }
        self.credentials.supports_model(model)
    }

    pub fn temp_unschedulable_active(&self) -> bool {
        self.temp_unschedulable_until
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|until| until.with_timezone(&Utc) > Utc::now())
    }

    pub fn rate_limit_active(&self) -> bool {
        self.rate_limit_reset_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|until| until.with_timezone(&Utc) > Utc::now())
    }

    pub fn overload_active(&self) -> bool {
        self.overload_until
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|until| until.with_timezone(&Utc) > Utc::now())
    }
}

#[derive(Debug, Clone)]
pub struct UsageLogDraft {
    pub api_key_id: Option<i64>,
    pub group_id: Option<i64>,
    pub account_id: u64,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub stream: bool,
    pub status: String,
    pub http_status: Option<u16>,
    pub error_kind: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageLogsQuery {
    #[serde(default = "default_page")]
    pub page: usize,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub account_id: Option<u64>,
    #[serde(default)]
    pub group_id: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLogsResponse {
    pub logs: Vec<UsageLogItem>,
    pub page: usize,
    pub page_size: usize,
    pub total_items: usize,
    pub total_pages: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLogItem {
    pub id: i64,
    pub api_key_id: Option<i64>,
    pub group_id: Option<i64>,
    pub group_name: Option<String>,
    pub account_id: u64,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub stream: bool,
    pub status: String,
    pub http_status: Option<u16>,
    pub error_kind: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub duration_ms: Option<i64>,
    pub created_at: String,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    50
}
