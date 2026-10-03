/* eslint-disable react-refresh/only-export-components -- 对话框与它的时序契约同文件：契约要被单测直接引用，拆成两个同名模块会撞 import 解析 */
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

/**
 * WSL 核心安装对话框的时序契约（从组件里抽出的纯逻辑，便于单测）。
 *
 * 顺序要求（PLAN §U6.3 / R-U9-1）：**先完成进度订阅，再发起安装**。
 * - 订阅失败：不发起安装，错误进入对话框展示；
 * - 订阅成功后已被卸载：不发起安装（迟到的 unlisten 立即注销）；
 * - 订阅成功且仍在挂载：只发起一次安装，成功回调 `onConfirm`，失败回调 `onError`。
 */
export interface WslCoreInstallRun {
  /** 订阅安装进度（返回注销函数），语义同 `listen('install-progress')` */
  subscribe: (handler: (payload: InstallProgress) => void) => Promise<() => void>
  /** 进度事件回调（调用方在此做 `type === 'wsl-core'` 过滤与状态更新） */
  onProgress: (payload: InstallProgress) => void
  /** 实际安装动作 */
  install: () => Promise<WslCoreProbe>
  /** 是否已被卸载（组件在 cleanup 里置位） */
  isCancelled: () => boolean
  /** 安装成功 */
  onConfirm: (probe: WslCoreProbe) => void
  /** 订阅失败或安装失败 */
  onError: (message: string) => void
}

/**
 * 执行「订阅 → 确认未卸载 → 安装」的单一异步链。
 * @returns cleanup：注销监听；卸载早于订阅完成时，会在订阅 resolve 后立即注销且不会安装。
 */
export function runWslCoreInstall(run: WslCoreInstallRun): () => void {
  let unlisten: (() => void) | undefined

  run.subscribe((payload) => {
    run.onProgress(payload)
  }).then(
    (fn) => {
      if (run.isCancelled()) {
        // 卸载早于 listen resolve：立即注销，且不发起安装
        fn()
        return
      }
      unlisten = fn
      return run.install().then(
        (probe) => {
          if (!run.isCancelled())
            run.onConfirm(probe)
        },
        (err) => {
          if (!run.isCancelled())
            run.onError(String(err))
        },
      )
    },
    (err) => {
      // 订阅失败：不发起安装，让用户看到失败
      if (!run.isCancelled())
        run.onError(String(err))
    },
  )

  return () => {
    unlisten?.()
  }
}

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
  // 因此用 runWslCoreInstall 保证「订阅 resolve → 未卸载 → 才发起安装」的单一异步链；
  // 订阅失败不发起安装并把错误留在对话框内；卸载早于 listen resolve 时立即注销。
  useEffect(() => {
    if (!disclosure.visible)
      return
    let cancelled = false
    const cleanup = runWslCoreInstall({
      subscribe: handler => listen<InstallProgress>('install-progress', (e) => {
        // 只处理 WSL 核心的事件：核心版本下载（app-*）与首装安装同用该通道，
        // 两边各自按 type 过滤才不会互相吞掉进度（方案 W5.3）。
        if (e.payload.type === 'wsl-core')
          handler(e.payload)
      }),
      onProgress: (payload) => {
        setPercentage(prev => Math.max(prev, payload.percentage))
        if (payload.log)
          setLogs(prev => [...prev, payload.log].slice(-5))
      },
      install: () => props.runInstall(),
      isCancelled: () => cancelled,
      onConfirm: probe => disclosure.confirm(probe),
      onError: message => setErrorMsg(message),
    })

    return () => {
      cancelled = true
      cleanup()
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
