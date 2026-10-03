import type { PropsWithOverlays } from '@overlastic/react'
import type { InstallProgress } from '@/store/modules/harness/types'
import type { WslCoreProbe } from '@/types'
import { AlertDialog, Button, Spinner } from '@heroui/react'
import { useDisclosure } from '@overlastic/react'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { If } from 'react-if-lite'
import { Panel } from '@/components/panel'

/** WSL 核心安装 / 更新进度对话框（只消费 `type === 'wsl-core'` 的进度事件） */
export interface WslCoreInstallDialogProps extends PropsWithOverlays {
  distro: string
  /** 当前已装版本（展示用） */
  version?: string
  /** 实际安装动作（返回最新探测结果） */
  runInstall: () => Promise<WslCoreProbe>
}

export function WslCoreInstallDialog(props: WslCoreInstallDialogProps) {
  const disclosure = useDisclosure({ props, delay: 300 })
  const { t } = useTranslation()

  const [percentage, setPercentage] = useState(0)
  const [logs, setLogs] = useState<string[]>([])
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const error = errorMsg != null

  // keep:effect 订阅必须先于安装命令完成：install-progress 的首次进度不能丢，
  // 因此保留 listen→runInstall→清理的 Promise 生命周期（不能用 useListen 后立即开装）。
  // 卸载早于 listen resolve 时立即注销；失败详情留在对话框内展示。
  useEffect(() => {
    if (!disclosure.visible)
      return
    let unlisten: (() => void) | undefined
    let cancelled = false
    listen<InstallProgress>('install-progress', (e) => {
      if (cancelled)
        return
      const payload = e.payload
      // 只处理 WSL 核心的事件：核心版本下载（app-*）与首装安装同用该通道，
      // 两边各自按 type 过滤才不会互相吞掉进度（方案 W5.3）。
      if (payload.type !== 'wsl-core')
        return
      setPercentage(prev => Math.max(prev, payload.percentage))
      if (payload.log)
        setLogs(prev => [...prev, payload.log].slice(-5))
    })
      .then((fn) => {
        if (cancelled)
          fn()
        else unlisten = fn
      })
      .catch(() => {})

    props.runInstall()
      .then((probe) => {
        if (!cancelled)
          disclosure.confirm(probe)
      })
      .catch((err) => {
        if (!cancelled)
          setErrorMsg(String(err))
      })

    return () => {
      cancelled = true
      unlisten?.()
    }
    // eslint-disable-next-line react/exhaustive-deps -- 仅打开时执行一次
  }, [disclosure.visible])

  const status = error ? 'danger' : 'default'

  return (
    <AlertDialog onOpenChange={disclosure.cancel} isOpen={disclosure.visible}>
      <AlertDialog.Backdrop>
        <AlertDialog.Container>
          <AlertDialog.Dialog className="sm:max-w-[420px]">
            <If cond={error}>
              <AlertDialog.CloseTrigger />
            </If>
            <AlertDialog.Header>
              <AlertDialog.Icon status={status} />
              <AlertDialog.Heading>
                {error ? t('wsl_core.install_failed') : t('wsl_core.installing')}
              </AlertDialog.Heading>
            </AlertDialog.Header>
            <AlertDialog.Body>
              <If
                cond={!error}
                else={(
                  <div className="flex flex-col gap-3">
                    <p className="break-all font-mono text-xs leading-[1.7] text-danger">{errorMsg}</p>
                    <Panel.Progress logs={logs} />
                  </div>
                )}
              >
                <div className="flex flex-col items-start gap-3">
                  <div className="flex items-center gap-2">
                    <Spinner size="sm" color="current" />
                    <span className="text-xs text-muted">{t('wsl_core.installing_hint', { distro: props.distro })}</span>
                  </div>
                  <Panel.Progress percentage={percentage} logs={logs} />
                </div>
              </If>
            </AlertDialog.Body>
            <AlertDialog.Footer>
              <If cond={error}>
                <Button variant="tertiary" onPress={disclosure.cancel}>
                  {t('wsl_core.install_close')}
                </Button>
              </If>
            </AlertDialog.Footer>
          </AlertDialog.Dialog>
        </AlertDialog.Container>
      </AlertDialog.Backdrop>
    </AlertDialog>
  )
}
