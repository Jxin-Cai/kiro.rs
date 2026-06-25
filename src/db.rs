use std::path::Path;

use anyhow::Context;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::kiro::model::credentials::KiroCredentials;
use crate::kiro::postgres_account_store::PostgresAccountStore;
use crate::model::config::Config;

const INIT_SQL: &str = include_str!("../migrations/0001_init.sql");

pub async fn init_database(
    config: &Config,
    credentials_path: &Path,
    credentials: &[KiroCredentials],
) -> anyhow::Result<PgPool> {
    let database_url = database_url(config)?;

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .with_context(|| format!("连接数据库失败: {database_url}"))?;

    sqlx::raw_sql(INIT_SQL).execute(&pool).await?;

    if config.auto_migrate_json {
        let store = PostgresAccountStore::new(pool.clone());
        store
            .import_from_json_if_needed(credentials_path, credentials)
            .await?;
    }

    Ok(pool)
}

pub fn database_url(config: &Config) -> anyhow::Result<String> {
    if let Some(url) = config
        .database_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
    {
        return Ok(url.to_string());
    }

    std::env::var("DATABASE_URL").context("未配置 database_url，且环境变量 DATABASE_URL 不存在")
}
