import { useMemo, useState } from 'react'
import { Copy, KeyRound, Plus, RefreshCw, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  useApiKeys,
  useCreateApiKey,
  useCreateGroup,
  useDeleteApiKey,
  useDeleteGroup,
  useGroups,
  useRotateApiKey,
  useUpdateApiKey,
  useUsageLogs,
} from '@/hooks/use-credentials'
import { cn, extractErrorMessage } from '@/lib/utils'
import type { ApiKeyRecord, GroupRecord, UsageLogItem } from '@/types/api'

function formatDateTime(value?: string | null) {
  if (!value) return '-'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString()
}

function groupName(groups: GroupRecord[] | undefined, id?: number | null) {
  if (!id) return '默认组'
  return groups?.find(group => group.id === id)?.name || `分组 #${id}`
}

function copyPlainKey(value: string) {
  navigator.clipboard.writeText(value)
    .then(() => toast.success('已复制 API Key'))
    .catch(() => toast.error('复制失败，请手动选择复制'))
}

export function GroupsPanel() {
  const { data: groups = [], isLoading } = useGroups()
  const createGroup = useCreateGroup()
  const deleteGroup = useDeleteGroup()
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')

  const handleCreate = () => {
    const trimmed = name.trim()
    if (!trimmed) {
      toast.error('分组名称不能为空')
      return
    }

    createGroup.mutate(
      { name: trimmed, description: description.trim() || undefined },
      {
        onSuccess: () => {
          toast.success('分组已创建')
          setName('')
          setDescription('')
        },
        onError: error => toast.error('创建失败: ' + extractErrorMessage(error)),
      }
    )
  }

  const handleDelete = (group: GroupRecord) => {
    if (group.isDefault) {
      toast.error('默认分组不能删除')
      return
    }
    if (!confirm(`确定删除分组「${group.name}」吗？`)) return

    deleteGroup.mutate(group.id, {
      onSuccess: response => toast.success(response.message),
      onError: error => toast.error('删除失败: ' + extractErrorMessage(error)),
    })
  }

  return (
    <div className="grid min-h-0 gap-4 lg:grid-cols-[360px_1fr]">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">新建分组</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <Input
            value={name}
            onChange={event => setName(event.target.value)}
            placeholder="分组名称"
          />
          <Input
            value={description}
            onChange={event => setDescription(event.target.value)}
            placeholder="描述（可选）"
          />
          <Button onClick={handleCreate} disabled={createGroup.isPending} className="w-full">
            <Plus className="h-4 w-4" />
            创建分组
          </Button>
        </CardContent>
      </Card>

      <Card className="min-h-0 overflow-hidden">
        <CardHeader>
          <CardTitle className="text-base">账号分组</CardTitle>
        </CardHeader>
        <CardContent className="min-h-0 overflow-auto">
          {isLoading ? (
            <div className="py-8 text-center text-sm text-muted-foreground">加载中...</div>
          ) : groups.length === 0 ? (
            <div className="py-8 text-center text-sm text-muted-foreground">暂无分组</div>
          ) : (
            <div className="space-y-2">
              {groups.map(group => (
                <div key={group.id} className="flex items-center justify-between gap-3 rounded-md border p-3">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-sm font-medium">{group.name}</span>
                      {group.isDefault && <Badge variant="secondary">默认</Badge>}
                      <Badge variant={group.status === 'active' ? 'success' : 'outline'}>{group.status}</Badge>
                    </div>
                    <div className="mt-1 truncate text-xs text-muted-foreground">
                      {group.description || '无描述'} · {group.accountCount} 个账号 · 优先级 {group.priority}
                    </div>
                  </div>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => handleDelete(group)}
                    disabled={deleteGroup.isPending || group.isDefault}
                  >
                    <Trash2 className="h-4 w-4" />
                    删除
                  </Button>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

export function ApiKeysPanel() {
  const { data: groups = [] } = useGroups()
  const { data: apiKeys = [], isLoading } = useApiKeys()
  const createApiKey = useCreateApiKey()
  const updateApiKey = useUpdateApiKey()
  const deleteApiKey = useDeleteApiKey()
  const rotateApiKey = useRotateApiKey()
  const [name, setName] = useState('')
  const [groupId, setGroupId] = useState('')
  const [plainKey, setPlainKey] = useState<string | null>(null)

  const activeGroups = useMemo(
    () => groups.filter(group => group.status === 'active'),
    [groups]
  )

  const handleCreate = () => {
    const trimmed = name.trim()
    if (!trimmed) {
      toast.error('API Key 名称不能为空')
      return
    }

    createApiKey.mutate(
      { name: trimmed, groupId: groupId ? Number(groupId) : undefined },
      {
        onSuccess: response => {
          setPlainKey(response.apiKey)
          setName('')
          setGroupId('')
          toast.success(response.message)
        },
        onError: error => toast.error('创建失败: ' + extractErrorMessage(error)),
      }
    )
  }

  const handleToggleStatus = (record: ApiKeyRecord) => {
    updateApiKey.mutate(
      { id: record.id, req: { status: record.status === 'active' ? 'disabled' : 'active' } },
      {
        onSuccess: () => toast.success('API Key 状态已更新'),
        onError: error => toast.error('更新失败: ' + extractErrorMessage(error)),
      }
    )
  }

  const handleRotate = (record: ApiKeyRecord) => {
    if (!confirm(`确定轮换 API Key「${record.name}」吗？旧 key 会立即失效。`)) return

    rotateApiKey.mutate(record.id, {
      onSuccess: response => {
        setPlainKey(response.apiKey)
        toast.success(response.message)
      },
      onError: error => toast.error('轮换失败: ' + extractErrorMessage(error)),
    })
  }

  const handleDelete = (record: ApiKeyRecord) => {
    if (!confirm(`确定删除 API Key「${record.name}」吗？`)) return

    deleteApiKey.mutate(record.id, {
      onSuccess: response => toast.success(response.message),
      onError: error => toast.error('删除失败: ' + extractErrorMessage(error)),
    })
  }

  return (
    <div className="grid min-h-0 gap-4 lg:grid-cols-[360px_1fr]">
      <div className="space-y-4">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">新建 API Key</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <Input
              value={name}
              onChange={event => setName(event.target.value)}
              placeholder="名称"
            />
            <select
              value={groupId}
              onChange={event => setGroupId(event.target.value)}
              className="h-10 w-full rounded-md border border-input bg-background px-3 text-sm"
            >
              <option value="">默认分组</option>
              {activeGroups.map(group => (
                <option key={group.id} value={group.id}>{group.name}</option>
              ))}
            </select>
            <Button onClick={handleCreate} disabled={createApiKey.isPending} className="w-full">
              <KeyRound className="h-4 w-4" />
              创建 API Key
            </Button>
          </CardContent>
        </Card>

        {plainKey && (
          <Card className="border-yellow-300 bg-yellow-50 dark:border-yellow-900 dark:bg-yellow-950/20">
            <CardHeader>
              <CardTitle className="text-base">请立即保存明文 Key</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="break-all rounded-md border bg-background p-3 font-mono text-xs">
                {plainKey}
              </div>
              <Button variant="outline" className="w-full" onClick={() => copyPlainKey(plainKey)}>
                <Copy className="h-4 w-4" />
                复制
              </Button>
            </CardContent>
          </Card>
        )}
      </div>

      <Card className="min-h-0 overflow-hidden">
        <CardHeader>
          <CardTitle className="text-base">API Keys</CardTitle>
        </CardHeader>
        <CardContent className="min-h-0 overflow-auto">
          {isLoading ? (
            <div className="py-8 text-center text-sm text-muted-foreground">加载中...</div>
          ) : apiKeys.length === 0 ? (
            <div className="py-8 text-center text-sm text-muted-foreground">暂无 API Key</div>
          ) : (
            <div className="space-y-2">
              {apiKeys.map(record => (
                <div key={record.id} className="rounded-md border p-3">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="truncate text-sm font-medium">{record.name}</span>
                        <Badge variant={record.status === 'active' ? 'success' : 'outline'}>{record.status}</Badge>
                      </div>
                      <div className="mt-1 text-xs text-muted-foreground">
                        {groupName(groups, record.groupId)} · 创建 {formatDateTime(record.createdAt)} · 最近使用 {formatDateTime(record.lastUsedAt)}
                      </div>
                    </div>
                    <div className="flex shrink-0 flex-wrap justify-end gap-1">
                      <Button size="sm" variant="outline" onClick={() => handleToggleStatus(record)} disabled={updateApiKey.isPending}>
                        {record.status === 'active' ? '禁用' : '启用'}
                      </Button>
                      <Button size="sm" variant="outline" onClick={() => handleRotate(record)} disabled={rotateApiKey.isPending}>
                        <RefreshCw className="h-4 w-4" />
                        轮换
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => handleDelete(record)} disabled={deleteApiKey.isPending}>
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

function usageStatusVariant(status: string) {
  if (status === 'success') return 'success'
  if (status === 'error') return 'destructive'
  return 'outline'
}

function UsageLogRow({ log }: { log: UsageLogItem }) {
  return (
    <tr className="border-b">
      <td className="px-3 py-2 text-sm">#{log.accountId}</td>
      <td className="px-3 py-2 text-sm">{log.groupId ? `#${log.groupId}` : '-'}</td>
      <td className="px-3 py-2 text-sm">{log.apiKeyId ? `#${log.apiKeyId}` : '全局 Key'}</td>
      <td className="px-3 py-2 text-sm">{log.model || '-'}</td>
      <td className="px-3 py-2 text-sm">{log.endpoint || '-'}</td>
      <td className="px-3 py-2 text-sm">
        <Badge variant={usageStatusVariant(log.status)}>{log.status}</Badge>
      </td>
      <td className="px-3 py-2 text-sm">{log.httpStatus || '-'}</td>
      <td className="px-3 py-2 text-sm">{log.durationMs === null || log.durationMs === undefined ? '-' : `${log.durationMs}ms`}</td>
      <td className="px-3 py-2 text-sm text-muted-foreground">{formatDateTime(log.createdAt)}</td>
    </tr>
  )
}

export function UsageLogsPanel() {
  const [page, setPage] = useState(1)
  const query = useMemo(() => ({ page, pageSize: 50 }), [page])
  const { data, isLoading, refetch } = useUsageLogs(query)
  const logs = data?.logs || []
  const totalPages = data?.totalPages || 1

  return (
    <Card className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <CardHeader className="flex-row items-center justify-between space-y-0">
        <CardTitle className="text-base">使用日志</CardTitle>
        <Button size="sm" variant="outline" onClick={() => refetch()}>
          <RefreshCw className="h-4 w-4" />
          刷新
        </Button>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
        <div className="min-h-0 flex-1 overflow-auto rounded-md border">
          <table className="w-full min-w-[980px] text-left">
            <thead className="sticky top-0 bg-muted text-xs uppercase text-muted-foreground">
              <tr>
                <th className="px-3 py-2 font-medium">账号</th>
                <th className="px-3 py-2 font-medium">分组</th>
                <th className="px-3 py-2 font-medium">API Key</th>
                <th className="px-3 py-2 font-medium">模型</th>
                <th className="px-3 py-2 font-medium">端点</th>
                <th className="px-3 py-2 font-medium">状态</th>
                <th className="px-3 py-2 font-medium">HTTP</th>
                <th className="px-3 py-2 font-medium">耗时</th>
                <th className="px-3 py-2 font-medium">时间</th>
              </tr>
            </thead>
            <tbody>
              {isLoading ? (
                <tr><td colSpan={9} className="px-3 py-8 text-center text-sm text-muted-foreground">加载中...</td></tr>
              ) : logs.length === 0 ? (
                <tr><td colSpan={9} className="px-3 py-8 text-center text-sm text-muted-foreground">暂无使用日志</td></tr>
              ) : (
                logs.map(log => <UsageLogRow key={log.id} log={log} />)
              )}
            </tbody>
          </table>
        </div>
        <div className="flex shrink-0 items-center justify-center gap-3">
          <Button size="sm" variant="outline" onClick={() => setPage(value => Math.max(1, value - 1))} disabled={page <= 1}>
            上一页
          </Button>
          <span className="text-sm text-muted-foreground">
            第 {page} / {totalPages} 页，共 {data?.totalItems || 0} 条
          </span>
          <Button size="sm" variant="outline" onClick={() => setPage(value => Math.min(totalPages, value + 1))} disabled={page >= totalPages}>
            下一页
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}

export function AdminTabButton({
  active,
  children,
  onClick,
}: {
  active: boolean
  children: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'rounded-md px-3 py-1.5 text-sm font-medium transition-colors',
        active
          ? 'bg-primary text-primary-foreground'
          : 'text-muted-foreground hover:bg-muted hover:text-foreground'
      )}
    >
      {children}
    </button>
  )
}
