//! Kiro 模型目录与模型 ID 规范化
//!
//! 对外 API 可能使用 Anthropic 风格模型名（含日期、thinking 后缀或连字符版本号），
//! Kiro 上游请求体使用较短的 canonical modelId。这里集中维护二者的映射，避免
//! converter、Admin API 与路由过滤各自维护一份逻辑。

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 前端模型选择器展示用模型选项。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    /// canonical Kiro model id，用于持久化和路由过滤。
    pub id: String,
    /// 展示名称。
    pub display_name: String,
    /// 上游/API 暴露的原始模型 ID（如有）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_id: Option<String>,
    /// 当前账号是否可选。
    pub available: bool,
    /// 不可选原因。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 将用户输入、Anthropic API 模型名或上游模型名规范化为 Kiro canonical modelId。
pub fn canonicalize_model_id(model: &str) -> Option<String> {
    let model_lower = model.trim().to_lowercase();
    if model_lower.is_empty() {
        return None;
    }

    if model_lower.contains("sonnet") {
        if model_lower.contains("4-6") || model_lower.contains("4.6") {
            Some("claude-sonnet-4.6".to_string())
        } else if model_lower.contains("4-5") || model_lower.contains("4.5") {
            Some("claude-sonnet-4.5".to_string())
        } else {
            None
        }
    } else if model_lower.contains("opus") {
        if model_lower.contains("4-8") || model_lower.contains("4.8") {
            Some("claude-opus-4.8".to_string())
        } else if model_lower.contains("4-7") || model_lower.contains("4.7") {
            Some("claude-opus-4.7".to_string())
        } else if model_lower.contains("4-6") || model_lower.contains("4.6") {
            Some("claude-opus-4.6".to_string())
        } else if model_lower.contains("4-5") || model_lower.contains("4.5") {
            Some("claude-opus-4.5".to_string())
        } else {
            None
        }
    } else if model_lower.contains("haiku") {
        Some("claude-haiku-4.5".to_string())
    } else {
        None
    }
}

/// 规范化模型列表：过滤非法项、去重并排序。
pub fn canonicalize_model_list(models: &[String]) -> Result<Vec<String>, String> {
    let mut result = BTreeSet::new();
    for model in models {
        let canonical = canonicalize_model_id(model)
            .ok_or_else(|| format!("不支持的模型: {}", model))?;
        result.insert(canonical);
    }
    Ok(result.into_iter().collect())
}

/// canonical 模型 ID 的展示名称。
pub fn display_name_for_canonical_model(id: &str) -> String {
    match id {
        "claude-opus-4.8" => "Claude Opus 4.8".to_string(),
        "claude-opus-4.7" => "Claude Opus 4.7".to_string(),
        "claude-opus-4.6" => "Claude Opus 4.6".to_string(),
        "claude-opus-4.5" => "Claude Opus 4.5".to_string(),
        "claude-sonnet-4.6" => "Claude Sonnet 4.6".to_string(),
        "claude-sonnet-4.5" => "Claude Sonnet 4.5".to_string(),
        "claude-haiku-4.5" => "Claude Haiku 4.5".to_string(),
        other => other.to_string(),
    }
}

/// 从上游 ListAvailableModels 响应中尽量提取模型列表。
///
/// 已观察到的第三方实现一般返回 `{ models: [...] , nextToken?: ... }`，模型项字段
/// 可能叫 `id` / `modelId` / `modelName` / `name`。这里保持宽松解析，以便兼容
/// Kiro 上游字段微调。
pub fn model_options_from_upstream_value(value: &serde_json::Value) -> Vec<ModelOption> {
    let candidates = value
        .get("models")
        .or_else(|| value.get("Models"))
        .or_else(|| value.get("data"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut seen = BTreeSet::new();
    let mut options = Vec::new();

    for item in candidates {
        let raw_id = item
            .get("id")
            .or_else(|| item.get("modelId"))
            .or_else(|| item.get("modelName"))
            .or_else(|| item.get("name"))
            .or_else(|| item.get("model"))
            .and_then(|v| v.as_str());

        let Some(raw_id) = raw_id else {
            continue;
        };
        let Some(canonical) = canonicalize_model_id(raw_id) else {
            continue;
        };
        if !seen.insert(canonical.clone()) {
            continue;
        }

        let display_name = item
            .get("displayName")
            .or_else(|| item.get("display_name"))
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| display_name_for_canonical_model(&canonical));

        options.push(ModelOption {
            id: canonical,
            display_name,
            upstream_id: Some(raw_id.to_string()),
            available: true,
            reason: None,
        });
    }

    options.sort_by(|a, b| a.id.cmp(&b.id));
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_api_model_ids() {
        assert_eq!(
            canonicalize_model_id("claude-sonnet-4-6-thinking"),
            Some("claude-sonnet-4.6".to_string())
        );
        assert_eq!(
            canonicalize_model_id("claude-opus-4-5-20251101"),
            Some("claude-opus-4.5".to_string())
        );
        assert_eq!(
            canonicalize_model_id("claude-haiku-4-5-20251001-thinking"),
            Some("claude-haiku-4.5".to_string())
        );
        assert_eq!(canonicalize_model_id("gpt-4"), None);
    }

    #[test]
    fn canonicalize_model_list_dedupes_and_sorts() {
        let models = vec![
            "claude-sonnet-4-6".to_string(),
            "claude-sonnet-4.6".to_string(),
            "claude-opus-4-7".to_string(),
        ];
        assert_eq!(
            canonicalize_model_list(&models).unwrap(),
            vec!["claude-opus-4.7".to_string(), "claude-sonnet-4.6".to_string()]
        );
    }

    #[test]
    fn parses_upstream_model_response() {
        let value = serde_json::json!({
            "models": [
                {"modelId": "claude-sonnet-4-6", "displayName": "Sonnet"},
                {"id": "claude-sonnet-4-6-thinking"},
                {"name": "claude-opus-4-7"},
                {"id": "unknown"}
            ]
        });
        let options = model_options_from_upstream_value(&value);
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].id, "claude-opus-4.7");
        assert_eq!(options[1].id, "claude-sonnet-4.6");
    }
}
