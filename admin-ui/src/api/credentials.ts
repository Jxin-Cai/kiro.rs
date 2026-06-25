import axios from 'axios'
import { storage } from '@/lib/storage'
import type {
  CredentialsStatusResponse,
  BalanceResponse,
  SuccessResponse,
  SetDisabledRequest,
  SetPriorityRequest,
  AddCredentialRequest,
  AddCredentialResponse,
  CredentialsQuery,
  CredentialModelsResponse,
  SetSupportedModelsRequest,
  GroupRecord,
  CreateGroupRequest,
  UpdateGroupRequest,
  SetAccountGroupsRequest,
  ApiKeyRecord,
  CreateApiKeyRequest,
  UpdateApiKeyRequest,
  CreateApiKeyResponse,
  UsageLogsQuery,
  UsageLogsResponse,
} from '@/types/api'

// 创建 axios 实例
const api = axios.create({
  baseURL: '/api/admin',
  headers: {
    'Content-Type': 'application/json',
  },
})

// 请求拦截器添加 API Key
api.interceptors.request.use((config) => {
  const apiKey = storage.getApiKey()
  if (apiKey) {
    config.headers['x-api-key'] = apiKey
  }
  return config
})

// 获取所有凭据状态
export async function getCredentials(
  query?: CredentialsQuery
): Promise<CredentialsStatusResponse> {
  const { data } = await api.get<CredentialsStatusResponse>('/credentials', {
    params: query,
  })
  return data
}

// 设置凭据禁用状态
export async function setCredentialDisabled(
  id: number,
  disabled: boolean
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(
    `/credentials/${id}/disabled`,
    { disabled } as SetDisabledRequest
  )
  return data
}

// 设置凭据优先级
export async function setCredentialPriority(
  id: number,
  priority: number
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(
    `/credentials/${id}/priority`,
    { priority } as SetPriorityRequest
  )
  return data
}

// 重置失败计数
export async function resetCredentialFailure(
  id: number
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(`/credentials/${id}/reset`)
  return data
}

// 强制刷新 Token
export async function forceRefreshToken(
  id: number
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(`/credentials/${id}/refresh`)
  return data
}

// 获取凭据余额
export async function getCredentialBalance(id: number): Promise<BalanceResponse> {
  const { data } = await api.get<BalanceResponse>(`/credentials/${id}/balance`)
  return data
}

// 获取当前凭据可用模型
export async function getCredentialModels(id: number): Promise<CredentialModelsResponse> {
  const { data } = await api.get<CredentialModelsResponse>(`/credentials/${id}/models`)
  return data
}

// 设置当前凭据可用模型
export async function setCredentialModels(
  id: number,
  models: string[]
): Promise<SuccessResponse> {
  const { data } = await api.put<SuccessResponse>(
    `/credentials/${id}/models`,
    { models } as SetSupportedModelsRequest
  )
  return data
}

// 添加新凭据
export async function addCredential(
  req: AddCredentialRequest
): Promise<AddCredentialResponse> {
  const { data } = await api.post<AddCredentialResponse>('/credentials', req)
  return data
}

// 导出指定凭据
export async function exportCredentials(
  ids: number[]
): Promise<AddCredentialRequest[]> {
  const { data } = await api.post<AddCredentialRequest[]>('/credentials/export', { ids })
  return data
}

// 删除凭据
export async function deleteCredential(id: number): Promise<SuccessResponse> {
  const { data } = await api.delete<SuccessResponse>(`/credentials/${id}`)
  return data
}

export async function setAccountGroups(
  id: number,
  groupIds: number[]
): Promise<CredentialsStatusResponse> {
  const { data } = await api.post<CredentialsStatusResponse>(
    `/credentials/${id}/groups`,
    { groupIds } as SetAccountGroupsRequest
  )
  return data
}

export async function listGroups(): Promise<GroupRecord[]> {
  const { data } = await api.get<GroupRecord[]>('/groups')
  return data
}

export async function createGroup(req: CreateGroupRequest): Promise<GroupRecord> {
  const { data } = await api.post<GroupRecord>('/groups', req)
  return data
}

export async function updateGroup(
  id: number,
  req: UpdateGroupRequest
): Promise<GroupRecord> {
  const { data } = await api.post<GroupRecord>(`/groups/${id}`, req)
  return data
}

export async function deleteGroup(id: number): Promise<SuccessResponse> {
  const { data } = await api.delete<SuccessResponse>(`/groups/${id}`)
  return data
}

export async function listApiKeys(): Promise<ApiKeyRecord[]> {
  const { data } = await api.get<ApiKeyRecord[]>('/api-keys')
  return data
}

export async function createApiKey(
  req: CreateApiKeyRequest
): Promise<CreateApiKeyResponse> {
  const { data } = await api.post<CreateApiKeyResponse>('/api-keys', req)
  return data
}

export async function updateApiKey(
  id: number,
  req: UpdateApiKeyRequest
): Promise<ApiKeyRecord> {
  const { data } = await api.post<ApiKeyRecord>(`/api-keys/${id}`, req)
  return data
}

export async function deleteApiKey(id: number): Promise<SuccessResponse> {
  const { data } = await api.delete<SuccessResponse>(`/api-keys/${id}`)
  return data
}

export async function rotateApiKey(id: number): Promise<CreateApiKeyResponse> {
  const { data } = await api.post<CreateApiKeyResponse>(`/api-keys/${id}/rotate`)
  return data
}

export async function listUsageLogs(
  query?: UsageLogsQuery
): Promise<UsageLogsResponse> {
  const { data } = await api.get<UsageLogsResponse>('/usage-logs', {
    params: query,
  })
  return data
}

// 获取负载均衡模式
export async function getLoadBalancingMode(): Promise<{ mode: 'priority' | 'balanced' }> {
  const { data } = await api.get<{ mode: 'priority' | 'balanced' }>('/config/load-balancing')
  return data
}

// 设置负载均衡模式
export async function setLoadBalancingMode(mode: 'priority' | 'balanced'): Promise<{ mode: 'priority' | 'balanced' }> {
  const { data } = await api.put<{ mode: 'priority' | 'balanced' }>('/config/load-balancing', { mode })
  return data
}
