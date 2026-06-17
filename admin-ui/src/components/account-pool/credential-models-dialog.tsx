import { useEffect, useMemo, useState } from 'react'
import { Loader2, RefreshCw } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useCredentialModels, useSetCredentialModels } from '@/hooks/use-credentials'
import { getCredentialDisplayName } from '@/lib/credential-format'
import { extractErrorMessage } from '@/lib/utils'
import type { CredentialStatusItem } from '@/types/api'

interface CredentialModelsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  credential: CredentialStatusItem | null
}

export function formatSupportedModels(models: string[] | undefined) {
  if (!models || models.length === 0) return '全部模型'
  if (models.length <= 2) return models.join(', ')
  return `${models.slice(0, 2).join(', ')} +${models.length - 2}`
}

export function CredentialModelsDialog({
  open,
  onOpenChange,
  credential,
}: CredentialModelsDialogProps) {
  const credentialId = credential?.id ?? null
  const { data, isLoading, error, refetch, isFetching } = useCredentialModels(credentialId, open)
  const setModels = useSetCredentialModels()
  const [limitModels, setLimitModels] = useState(false)
  const [selectedModels, setSelectedModels] = useState<Set<string>>(new Set())

  useEffect(() => {
    if (!open || !data) return
    setLimitModels(data.selectedModels.length > 0)
    setSelectedModels(new Set(data.selectedModels))
  }, [data, open])

  const availableModels = useMemo(
    () => (data?.models || []).filter(model => model.available),
    [data?.models]
  )

  const toggleModel = (id: string) => {
    setSelectedModels(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const handleSave = () => {
    if (!credential) return
    if (limitModels && selectedModels.size === 0) {
      toast.error('请选择至少一个模型，或切换为“不限制模型”')
      return
    }

    const models = limitModels ? Array.from(selectedModels) : []
    setModels.mutate(
      { id: credential.id, models },
      {
        onSuccess: (res) => {
          toast.success(res.message)
          onOpenChange(false)
        },
        onError: (err) => toast.error('保存失败: ' + extractErrorMessage(err)),
      }
    )
  }

  const saveDisabled =
    setModels.isPending || isLoading || Boolean(error) || (limitModels && selectedModels.size === 0)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[80vh] flex-col sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>设置账号可用模型</DialogTitle>
          <DialogDescription>
            {credential
              ? `账号 ${getCredentialDisplayName(credential)} · #${credential.id}`
              : '请选择账号'}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-[16rem] flex-1 overflow-y-auto pr-1">
          {isLoading ? (
            <div className="flex h-48 items-center justify-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              正在使用该账号查询真实上游模型列表...
            </div>
          ) : error ? (
            <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm">
              <div className="font-medium text-destructive">上游模型查询失败</div>
              <p className="mt-2 text-muted-foreground">{extractErrorMessage(error)}</p>
              <Button className="mt-4" size="sm" variant="outline" onClick={() => refetch()}>
                <RefreshCw className={isFetching ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
                重试
              </Button>
            </div>
          ) : (
            <div className="space-y-4">
              <div className="rounded-lg border p-3">
                <label className="flex cursor-pointer items-start gap-3">
                  <input
                    type="radio"
                    className="mt-1"
                    checked={!limitModels}
                    onChange={() => setLimitModels(false)}
                  />
                  <span>
                    <span className="block text-sm font-medium">不限制模型</span>
                    <span className="block text-xs text-muted-foreground">
                      支持该账号上游返回的全部可用模型。保存后配置为空数组。
                    </span>
                  </span>
                </label>
              </div>

              <div className="rounded-lg border p-3">
                <label className="flex cursor-pointer items-start gap-3">
                  <input
                    type="radio"
                    className="mt-1"
                    checked={limitModels}
                    onChange={() => setLimitModels(true)}
                  />
                  <span>
                    <span className="block text-sm font-medium">仅允许以下模型</span>
                    <span className="block text-xs text-muted-foreground">
                      请求路由时只会选择支持请求模型的账号。
                    </span>
                  </span>
                </label>

                <div className="mt-3 space-y-2">
                  {(data?.models || []).map(model => (
                    <label
                      key={model.id}
                      className="flex cursor-pointer items-start gap-3 rounded-md border bg-background p-3"
                    >
                      <Checkbox
                        checked={selectedModels.has(model.id)}
                        disabled={!limitModels || !model.available}
                        onCheckedChange={() => toggleModel(model.id)}
                      />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium" title={model.displayName}>
                          {model.displayName}
                        </span>
                        <span className="block truncate text-xs text-muted-foreground" title={model.id}>
                          {model.id}
                          {model.upstreamId && model.upstreamId !== model.id ? ` · ${model.upstreamId}` : ''}
                        </span>
                        {!model.available && model.reason && (
                          <span className="block text-xs text-destructive">{model.reason}</span>
                        )}
                      </span>
                    </label>
                  ))}

                  {availableModels.length === 0 && (
                    <div className="rounded-md border border-dashed p-4 text-center text-sm text-muted-foreground">
                      上游没有返回可选择的模型
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={handleSave} disabled={saveDisabled}>
            {setModels.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
            保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
