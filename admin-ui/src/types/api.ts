// 凭据状态响应
export interface CredentialsPagination {
  page: number
  pageSize: number
  totalItems: number
  totalPages: number
}

export interface CredentialsQuery {
  page: number
  pageSize: number
  search?: string
  status?: string
  authMethod?: string
  endpoint?: string
  groupId?: number
  schedulable?: boolean
  cooldown?: boolean
  sortKey?: string
  sortDirection?: 'asc' | 'desc'
}

export interface CredentialsStatusResponse {
  total: number
  available: number
  currentId: number
  credentials: CredentialStatusItem[]
  pagination: CredentialsPagination
}

export interface GroupSummary {
  id: number
  name: string
}

// 单个凭据状态
export interface CredentialStatusItem {
  id: number
  priority: number
  disabled: boolean
  failureCount: number
  isCurrent: boolean
  expiresAt: string | null
  authMethod: string | null
  hasProfileArn: boolean
  email?: string
  refreshTokenHash?: string
  apiKeyHash?: string
  maskedApiKey?: string
  successCount: number
  lastUsedAt: string | null
  hasProxy: boolean
  proxyUrl?: string
  refreshFailureCount: number
  disabledReason?: string
  endpoint: string
  supportedModels: string[]
  groups: GroupSummary[]
  schedulable: boolean
  status: string
  tempUnschedulableUntil?: string | null
  tempUnschedulableReason?: string | null
  rateLimitResetAt?: string | null
  overloadUntil?: string | null
}

// 余额响应
export interface BalanceResponse {
  id: number
  subscriptionTitle: string | null
  currentUsage: number
  usageLimit: number
  remaining: number
  usagePercentage: number
  nextResetAt: number | null
}

// 成功响应
export interface SuccessResponse {
  success: boolean
  message: string
}

// 错误响应
export interface AdminErrorResponse {
  error: {
    type: string
    message: string
  }
}

// 请求类型
export interface SetDisabledRequest {
  disabled: boolean
}

export interface SetPriorityRequest {
  priority: number
}

// 添加凭据请求
export interface AddCredentialRequest {
  refreshToken?: string
  authMethod?: 'social' | 'idc' | 'api_key'
  clientId?: string
  clientSecret?: string
  priority?: number
  disabled?: boolean
  region?: string
  authRegion?: string
  apiRegion?: string
  machineId?: string
  email?: string
  proxyUrl?: string
  proxyUsername?: string
  proxyPassword?: string
  kiroApiKey?: string
  endpoint?: string
  supportedModels?: string[]
}

export interface CredentialModelOption {
  id: string
  displayName: string
  upstreamId?: string
  available: boolean
  reason?: string
}

export interface CredentialModelsResponse {
  credentialId: number
  selectedModels: string[]
  models: CredentialModelOption[]
}

export interface SetSupportedModelsRequest {
  models: string[]
}

export interface SetAccountGroupsRequest {
  groupIds: number[]
}

export interface GroupRecord {
  id: number
  name: string
  description?: string | null
  status: string
  priority: number
  isDefault: boolean
  createdAt: string
  accountCount: number
}

export interface CreateGroupRequest {
  name: string
  description?: string
  priority?: number
}

export interface UpdateGroupRequest {
  name?: string
  description?: string
  status?: string
  priority?: number
}

export interface ApiKeyRecord {
  id: number
  name: string
  status: string
  groupId?: number | null
  groupName?: string | null
  lastUsedAt?: string | null
  quotaLimit: number
  quotaUsed: number
  expiresAt?: string | null
  createdAt: string
}

export interface CreateApiKeyRequest {
  name: string
  groupId?: number
  quotaLimit?: number
  expiresAt?: string
}

export interface UpdateApiKeyRequest {
  name?: string
  status?: string
  groupId?: number
  quotaLimit?: number
  expiresAt?: string
}

export interface CreateApiKeyResponse {
  success: boolean
  message: string
  apiKey: string
  record: ApiKeyRecord
}

export interface UsageLogsQuery {
  page: number
  pageSize: number
  accountId?: number
  groupId?: number
}

export interface UsageLogItem {
  id: number
  apiKeyId?: number | null
  groupId?: number | null
  accountId: number
  model?: string | null
  endpoint?: string | null
  stream: boolean
  status: string
  httpStatus?: number | null
  errorKind?: string | null
  inputTokens: number
  outputTokens: number
  durationMs?: number | null
  createdAt: string
}

export interface UsageLogsResponse {
  logs: UsageLogItem[]
  page: number
  pageSize: number
  totalItems: number
  totalPages: number
}

// 添加凭据响应
export interface AddCredentialResponse {
  success: boolean
  message: string
  credentialId: number
  email?: string
}
