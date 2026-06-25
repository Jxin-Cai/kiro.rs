-- PostgreSQL schema initialization

CREATE TABLE IF NOT EXISTS schema_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"')
);

CREATE TABLE IF NOT EXISTS accounts (
    id BIGSERIAL PRIMARY KEY,
    name TEXT,
    email TEXT,
    auth_method TEXT NOT NULL DEFAULT 'social',
    credential_json TEXT NOT NULL,
    credential_hash TEXT,
    priority BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'active',
    schedulable BOOLEAN NOT NULL DEFAULT TRUE,
    disabled_reason TEXT,
    failure_count BIGINT NOT NULL DEFAULT 0,
    refresh_failure_count BIGINT NOT NULL DEFAULT 0,
    success_count BIGINT NOT NULL DEFAULT 0,
    last_used_at TEXT,
    expires_at TEXT,
    subscription_title TEXT,
    endpoint TEXT,
    supported_models TEXT NOT NULL DEFAULT '[]',
    rate_limit_reset_at TEXT,
    overload_until TEXT,
    temp_unschedulable_until TEXT,
    temp_unschedulable_reason TEXT,
    created_at TEXT NOT NULL DEFAULT to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
    updated_at TEXT NOT NULL DEFAULT to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
    deleted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_accounts_schedulable
ON accounts(status, schedulable, priority)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_accounts_endpoint
ON accounts(endpoint)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_accounts_email
ON accounts(email)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_accounts_temp_until
ON accounts(temp_unschedulable_until)
WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_accounts_credential_hash_active
ON accounts(credential_hash)
WHERE deleted_at IS NULL AND credential_hash IS NOT NULL;

CREATE TABLE IF NOT EXISTS groups (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    priority BIGINT NOT NULL DEFAULT 0,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
    updated_at TEXT NOT NULL DEFAULT to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
    deleted_at TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_groups_name_active
ON groups(name)
WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS account_groups (
    account_id BIGINT NOT NULL,
    group_id BIGINT NOT NULL,
    priority BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
    PRIMARY KEY (account_id, group_id),
    FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY(group_id) REFERENCES groups(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_account_groups_group_priority
ON account_groups(group_id, priority);

CREATE TABLE IF NOT EXISTS api_keys (
    id BIGSERIAL PRIMARY KEY,
    key_hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    group_id BIGINT,
    last_used_at TEXT,
    quota_limit DOUBLE PRECISION NOT NULL DEFAULT 0,
    quota_used DOUBLE PRECISION NOT NULL DEFAULT 0,
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
    updated_at TEXT NOT NULL DEFAULT to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
    deleted_at TEXT,
    FOREIGN KEY(group_id) REFERENCES groups(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_api_keys_group_id
ON api_keys(group_id)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_api_keys_status
ON api_keys(status)
WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS usage_logs (
    id BIGSERIAL PRIMARY KEY,
    request_id TEXT,
    api_key_id BIGINT,
    group_id BIGINT,
    account_id BIGINT NOT NULL,
    model TEXT,
    endpoint TEXT,
    stream BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL,
    http_status BIGINT,
    error_kind TEXT,
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    cache_creation_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_tokens BIGINT NOT NULL DEFAULT 0,
    duration_ms BIGINT,
    created_at TEXT NOT NULL DEFAULT to_char(CURRENT_TIMESTAMP AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
    FOREIGN KEY(api_key_id) REFERENCES api_keys(id) ON DELETE SET NULL,
    FOREIGN KEY(group_id) REFERENCES groups(id) ON DELETE SET NULL,
    FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_usage_logs_account_created
ON usage_logs(account_id, created_at);

CREATE INDEX IF NOT EXISTS idx_usage_logs_group_created
ON usage_logs(group_id, created_at);

CREATE INDEX IF NOT EXISTS idx_usage_logs_api_key_created
ON usage_logs(api_key_id, created_at);

CREATE INDEX IF NOT EXISTS idx_usage_logs_model_created
ON usage_logs(model, created_at);

CREATE TABLE IF NOT EXISTS balance_cache (
    account_id BIGINT PRIMARY KEY,
    cached_at TEXT NOT NULL,
    data_json TEXT NOT NULL,
    FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

-- Insert default group if not exists
INSERT INTO groups (name, description, status, priority, is_default)
SELECT 'default', '默认账号组', 'active', 0, TRUE
WHERE NOT EXISTS (
    SELECT 1 FROM groups WHERE is_default = TRUE AND deleted_at IS NULL
);
