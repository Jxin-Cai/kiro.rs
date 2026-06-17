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

// 添加凭据响应
export interface AddCredentialResponse {
  success: boolean
  message: string
  credentialId: number
  email?: string
}
