import type { PropsWithOverlays } from '@overlastic/react'
import type { InstallProgress } from '@/store/modules/harness/types'
import { AlertDialog, Button, Chip, Description, Label, ListBox, Select, Spinner } from '@heroui/react'
import { useDisclosure, useOverlay } from '@overlastic/react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { If } from 'react-if-lite'
import { useStore } from 'valtio-define'
import { useAppConfig } from '@/hooks/use-app-config'
import { store } from '@/store'
import { silence } from '@/utils/silence'
import { toast } from '@/utils/toast'
import { useDshCores } from '../hooks/use-dsh-cores'
import { Item } from './item'
import { Modal } from './modal'
import { PanelHeader } from './panel-header'
import { PanelProgress } from './panel-progress'

/** Rust 侧 `service::wsl_core::exec::WslDistro` 的序列化形态 */
interface WslDistro {
  name: string
  state: string
  version: string
  isDefault: boolean
}

/** Rust 侧 `service::wsl_core::probe::WslCoreProbe` 的序列化形态 */
interface WslCoreProbe {
  distro: string
  node: string | null
  nodeVersion: string | null
  dsh: string | null
  dshVersion: string | null
  npmRoot: string | null
  /** 受控运行时目录（Linux 绝对路径；安装/更新会整体重建这个目录） */
  runtimeDir: string
  home: string
  skipAuthReady: boolean
}

/**
 * 「WSL 核心」面板（方案 W5；R-W5-2/3/4 返修）。
 *
 * 发行版下拉 + 状态行（已确认发行版挂载即探测）+ 安装 / 更新（WSL 运行中先停服，
 * 装完走前端完整 `store.harness.restart()`）+ 「使用此核心」（切换后复用核心面板的
 * 自动重启）+ 只读提示（数据目录与 Windows 侧不互通、内置插件不注入、凭据需单独
 * 导入）。
 *
 * 候选与已确认分离（R-W5-3）：下拉可默认显示 `isDefault` 候选，但只有显式选择 /
 * 探测 / 安装 / 导入才会写 `wsl_distro`；打开设置不写 store、不进发行版探测。
 * 运行中限制只针对 WSL 来源（`wslRunning = serviceRunning && active`，R-W5-2），
 * Windows 核心运行时仍可配置 WSL 且不打断当前会话。
 *
 * 进度事件只处理 `type === 'wsl-core'` 的 `install-progress`：核心版本下载
 * （`type` 为其它值）与首装安装同时进行时各走各的对话框，互不干扰。
 */
export function ConfigWslCore() {
  const { t } = useTranslation()
  const { serviceRunning } = useStore(store.harness)
  const { data: config, refetch: refreshConfig } = useAppConfig()
  const { cores, refreshCores } = useDshCores()

  const [confirmHolder, openConfirm] = useOverlay(Modal, { type: 'holder' })
  const [installHolder, openInstall] = useOverlay(WslCoreInstallDialog, { type: 'holder' })

  const [distros, setDistros] = useState<WslDistro[]>([])
  const [distrosError, setDistrosError] = useState('')
  const [probing, setProbing] = useState(false)
  const [busy, setBusy] = useState(false)
  const [probe, setProbe] = useState<WslCoreProbe | null>(null)
  const [probeError, setProbeError] = useState('')
  /** 下拉里显示的候选发行版：与已确认配置分离，未持久化（R-W5-3） */
  const [candidate, setCandidate] = useState('')

  /** 已确认（持久化）的发行版 */
  const distro = config?.wsl_distro ?? ''
  /** 下拉显示值：已确认优先，否则候选 */
  const selected = distro || candidate
  /** 只认与当前显示目标一致的探测结果，避免切换后展示上一个发行版的数据 */
  const current = probe?.distro === selected ? probe : null
  const wslCore = cores.find(core => core.source === 'wsl')
  const active = wslCore?.active ?? false
  /**
   * 运行中限制只针对「WSL 来源」正在运行（R-W5-2）：`serviceRunning` 不区分来源，
   * Windows 核心运行时必须仍可选择 / 探测 / 安装 WSL，不打断当前会话。
   */
  const wslRunning = serviceRunning && active
  const ready = !!current?.dsh && current.skipAuthReady
  const installed = !!current?.dsh
  const nodeMissing = !!current && !current.node
  const patchMissing = !!current?.dsh && !current.skipAuthReady

  /** 探测发行版内的 Node / dsh（挂载即调用；进程内缓存重启即空，必须现场探测） */
  async function runProbe(target: string) {
    if (!target)
      return
    setProbing(true)
    setProbeError('')
    try {
      setProbe(await invoke<WslCoreProbe>('probe_wsl_core', { distro: target }))
      // 核心列表的 WSL 行（`dir` / `version` / `present`）也取自后端探测缓存，
      // 探测完成后刷新一次——否则首次选定发行版时该行尚无缓存，「数据目录」会
      // 一直停在占位符，直到用户手动点「刷新」。
      await refreshCores().catch(() => {})
    }
    catch (err) {
      console.error('[ConfigWslCore] probe failed:', err)
      setProbe(null)
      setProbeError(String(err))
    }
    finally {
      setProbing(false)
    }
  }

  // 挂载：拉发行版列表；候选默认显示 `isDefault`（回落首项），但不写配置（R-W5-3）
  useEffect(() => {
    let cancelled = false
    invoke<WslDistro[]>('list_wsl_distros')
      .then((list) => {
        if (cancelled)
          return
        setDistros(list)
        setDistrosError('')
        setCandidate(prev => prev || (list.find(item => item.isDefault) ?? list[0])?.name || '')
      })
      .catch((err) => {
        console.error('[ConfigWslCore] list distros failed:', err)
        if (!cancelled)
          setDistrosError(String(err))
      })
    return () => {
      cancelled = true
    }
  }, [])

  // 仅已确认的发行版在挂载 / 变化时现场探测（进程内缓存重启即空，必须现场探测）
  useEffect(() => {
    if (distro)
      void runProbe(distro)
    // eslint-disable-next-line react/exhaustive-deps -- 仅按发行版变化探测，runProbe 每次渲染都会重建
  }, [distro])

  /** 持久化发行版为已确认配置（WSL 运行中会被后端 `WSL_DISTRO_LOCKED` 拒绝） */
  async function persistDistro(name: string) {
    try {
      await invoke('update_app_config', { wslDistro: name })
      await refreshConfig()
      return true
    }
    catch (err) {
      console.error('[ConfigWslCore] set distro failed:', err)
      const message = String(err)
      toast(message.includes('WSL_DISTRO_LOCKED') ? t('wsl_core.distro_locked') : t('wsl_core.probe_failed'), {})
      return false
    }
  }

  /** 用户在下拉里明确选择：只有显式操作才会写 `wsl_distro`（R-W5-3） */
  async function onSelectDistro(name: string) {
    if (!name || busy || name === distro)
      return
    if (wslRunning) {
      toast(t('wsl_core.distro_locked'), {})
      return
    }
    await persistDistro(name)
  }

  /** 「重新检测」：候选未确认时先确认，随后由 `[distro]` effect 现场探测 */
  async function onProbeSelected() {
    const target = selected
    if (!target || busy || probing)
      return
    if (target === distro) {
      await runProbe(target)
      return
    }
    if (wslRunning) {
      toast(t('wsl_core.distro_locked'), {})
      return
    }
    await persistDistro(target)
  }

  /** 安装 / 更新：确认 →（WSL 运行中则停服）→ 安装 →（原先在跑则前端完整重启） */
  async function onInstall() {
    const target = selected
    if (busy || !target)
      return
    // 未持久化的候选：安装动作即确认，后续全部使用当次目标（R-W5-3）
    if (target !== distro) {
      if (wslRunning) {
        toast(t('wsl_core.distro_locked'), {})
        return
      }
      if (!await persistDistro(target))
        return
    }
    // 确认框展示旧 / 新兼容基线与本应用受控 runtime 路径（v8）：更新只整体重建这个
    // 私有运行时目录，不卸载 / 不改动用户在发行版内的全局 dsh 与既有数据。
    const recommended = wslCore?.recommendedVersion ?? ''
    const runtimePath = current?.runtimeDir ?? ''
    const crossVersion = !!current?.dshVersion && !!recommended && current.dshVersion !== recommended
    try {
      await openConfirm({
        status: 'warning',
        title: crossVersion
          ? t('wsl_core.reinstall_confirm_title')
          : installed
            ? t('wsl_core.update_confirm_title')
            : t('wsl_core.install_confirm_title'),
        description: (
          <div className="space-y-1">
            <p>
              {crossVersion
                ? t('wsl_core.reinstall_confirm_desc', {
                    from: current?.dshVersion ?? '',
                    to: recommended,
                  })
                : installed
                  ? t('wsl_core.update_confirm_desc')
                  : t('wsl_core.install_confirm_desc', { distro: target })}
            </p>
            <If cond={runtimePath !== ''}>
              <p className="text-muted">
                {t('wsl_core.runtime_dir_hint', { path: runtimePath })}
              </p>
            </If>
          </div>
        ),
        confirmText: installed ? t('wsl_core.update') : t('wsl_core.install'),
      })
    }
    catch (e) {
      silence(e, 'wsl install: dialog cancelled')
      return
    }

    setBusy(true)
    // 只有 WSL 来源在跑才停服；Windows 核心运行时不打断当前会话（R-W5-2）
    const wasRunning = wslRunning
    let ok = false
    try {
      // 先停服再进进度对话框：否则前端 shutdown 会把进度面板一起关掉（W5.3）
      if (wasRunning)
        await invoke('shutdown_harness')
      await openInstall({
        distro: target,
        version: current?.dshVersion ?? undefined,
        runInstall: () => invoke<WslCoreProbe>('install_wsl_core', { distro: target }),
      })
      ok = true
    }
    catch (err) {
      // 失败详情已在进度对话框内展示
      console.error('[ConfigWslCore] install failed:', err)
    }
    finally {
      // 原先在跑就恢复服务：走前端完整重启流程（boot → runtime → readiness），
      // 端口 heal 后 iframe 地址会同步；失败时也恢复，避免把用户留在停服状态（R-W5-4）
      if (wasRunning) {
        try {
          await store.harness.restart()
        }
        catch (e) {
          silence(e, 'wsl install: restart failure already shown by app error state')
        }
      }
      setBusy(false)
    }

    if (ok) {
      toast(t('wsl_core.installed_toast'), { variant: 'accent' })
      // 完整 restart 会按既有行为关闭设置面板；面板仍在时刷新探测与核心列表
      if (!wasRunning)
        await runProbe(target)
    }
  }

  /** 使用此核心：切换后复用核心面板的自动重启流程 */
  async function onUse() {
    // 候选尚未确认时探测结果不对应，禁止切换
    if (busy || active || !ready || selected !== distro)
      return
    try {
      await openConfirm({
        status: 'warning',
        title: t('core.switch_confirm_title'),
        description: (
          <p>
            {t('core.switch_confirm_desc', { version: current?.dshVersion ?? `WSL · ${distro}` })}
          </p>
        ),
      })
    }
    catch (e) {
      silence(e, 'wsl switch: dialog cancelled')
      return
    }
    setBusy(true)
    try {
      await invoke('set_active_core', { id: 'wsl' })
      const key = toast(t('core.activate_toast', { version: current?.dshVersion ?? `WSL · ${distro}` }), {
        variant: 'accent',
        description: t('core.switch_restart_hint'),
        timeout: 10_000,
      })
      try {
        await store.harness.restart()
        toast.close(key)
      }
      catch (e) {
        silence(e, 'wsl switch: restart failure already shown by app error state')
      }
    }
    catch (err) {
      console.error('[ConfigWslCore] switch failed:', err)
      toast(t('wsl_core.use_failed'), {})
    }
    finally {
      setBusy(false)
    }
  }

  /** 从 Windows 侧导入 API Key（D-W5-1：显式点击、单向、不自动同步） */
  async function onImportKey() {
    const target = selected
    if (busy || !target)
      return
    // 未持久化的候选：导入动作即确认（R-W5-3）
    if (target !== distro) {
      if (wslRunning) {
        toast(t('wsl_core.distro_locked'), {})
        return
      }
      if (!await persistDistro(target))
        return
    }
    try {
      await openConfirm({
        status: 'warning',
        title: t('wsl_core.import_key_confirm_title'),
        description: <p>{t('wsl_core.import_key_confirm_desc', { distro: target })}</p>,
        confirmText: t('wsl_core.import_key'),
      })
    }
    catch (e) {
      silence(e, 'wsl import key: dialog cancelled')
      return
    }
    setBusy(true)
    try {
      const imported = await invoke<boolean>('import_wsl_credentials', { distro: target })
      toast(imported ? t('wsl_core.import_key_toast') : t('wsl_core.import_key_uptodate'), {})
    }
    catch (err) {
      console.error('[ConfigWslCore] import key failed:', err)
      toast(t('wsl_core.import_key_failed'), {})
    }
    finally {
      setBusy(false)
    }
  }

  async function onRevealDir() {
    if (!wslCore?.dir)
      return
    try {
      await invoke('reveal_in_folder', { path: wslCore.dir })
    }
    catch (err) {
      console.error('[ConfigWslCore] reveal dir failed:', err)
      toast(t('core.open_dir_failed'), {})
    }
  }

  return (
    <div className="space-y-3">
      <PanelHeader title={t('wsl_core.title')} description={t('wsl_core.tooltip')} />

      {/* 发行版选择 + 状态 */}
      <div className="flex flex-col gap-3 rounded-md border border-divider p-3">
        <div className="flex items-center gap-3">
          <Label className="shrink-0 text-sm font-medium text-ink">{t('wsl_core.distro')}</Label>
          <Select
            variant="secondary"
            selectedKey={selected || null}
            placeholder={t('wsl_core.distro_missing')}
            isDisabled={busy || probing || distros.length === 0 || wslRunning}
            onSelectionChange={key => onSelectDistro(String(key))}
            className="w-[220px]"
            aria-label={t('wsl_core.distro')}
          >
            <Select.Trigger className="rounded-md min-h-8! h-8 py-0 items-center">
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover className="rounded-md">
              <ListBox>
                {distros.map(item => (
                  <ListBox.Item key={item.name} id={item.name} textValue={item.name}>
                    {item.name}
                  </ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
          <Button
            size="sm"
            variant="tertiary"
            className="h-8 shrink-0 rounded-md text-xs"
            isDisabled={busy || probing || !selected}
            onPress={onProbeSelected}
          >
            <If cond={probing} then={<Spinner size="sm" color="current" />} />
            {t('wsl_core.probe')}
          </Button>
        </div>

        {/* WSL 核心运行中：发行版下拉被禁用时的说明 */}
        <If cond={wslRunning}>
          <Description className="text-xs text-muted">{t('wsl_core.distro_locked')}</Description>
        </If>
        {/* 列表查询失败与「查询成功但为空」分开提示（R-W5-5） */}
        <If cond={distrosError !== '' && distros.length === 0}>
          <Description className="text-xs text-danger">{t('wsl_core.distros_failed')}</Description>
        </If>
        <If cond={distrosError === '' && distros.length === 0}>
          <Description className="text-xs text-muted">{t('wsl_core.distro_empty')}</Description>
        </If>

        {/* 状态行：Node / dsh / 补丁 */}
        <If cond={!selected}>
          <Description className="text-xs text-muted">{t('wsl_core.distro_missing')}</Description>
        </If>
        <If cond={!!selected}>
          <div className="flex flex-wrap items-center gap-2">
            <If cond={probing && !current}>
              <Spinner size="sm" color="current" />
            </If>
            <If cond={nodeMissing}>
              <Chip size="sm" variant="soft" color="danger" className="shrink-0 font-medium">
                {t('wsl_core.node_missing')}
              </Chip>
            </If>
            <If cond={!!current?.node && !nodeMissing}>
              <Chip size="sm" variant="soft" color="default" className="shrink-0 font-medium">
                {`${t('wsl_core.node_version')} ${current?.nodeVersion ?? ''}`.trim()}
              </Chip>
            </If>
            <If cond={installed}>
              <Chip size="sm" variant="soft" color="accent" className="shrink-0 font-medium">
                {`${t('wsl_core.dsh_version')} ${current?.dshVersion ?? ''}`.trim()}
              </Chip>
            </If>
            <If cond={!!current && !installed}>
              <Chip size="sm" variant="soft" color="warning" className="shrink-0 font-medium">
                {t('wsl_core.not_installed')}
              </Chip>
            </If>
            <If cond={patchMissing}>
              <Chip size="sm" variant="soft" color="warning" className="shrink-0 font-medium">
                {t('wsl_core.skip_auth_missing')}
              </Chip>
            </If>
          </div>
        </If>
        <If cond={probeError !== ''}>
          <Description className="break-all font-mono text-xs text-danger">{probeError}</Description>
        </If>

        {/* 操作 */}
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            className="h-8 rounded-md text-xs"
            isDisabled={busy || probing || !selected || nodeMissing || distros.length === 0}
            onPress={onInstall}
          >
            {installed ? t('wsl_core.update') : t('wsl_core.install')}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            className="h-8 rounded-md text-xs"
            isDisabled={busy || active || !ready || selected !== distro}
            onPress={onUse}
          >
            {active ? t('wsl_core.active') : t('wsl_core.use')}
          </Button>
        </div>
      </div>

      {/* 只读提示：数据目录 / 内置插件 / 与 Windows 侧隔离 */}
      <Item
        left={(
          <div className="flex min-w-0 flex-col gap-1">
            <Description className="break-all font-mono text-xs text-muted">
              {t('wsl_core.data_dir_hint', { dir: wslCore?.dir || '—' })}
            </Description>
            <Description className="text-xs text-muted">{t('wsl_core.plugins_hint')}</Description>
            <Description className="text-xs text-muted">{t('wsl_core.isolation_hint')}</Description>
            <Description className="text-xs text-muted">{t('wsl_core.credentials_hint')}</Description>
          </div>
        )}
        right={(
          <>
            <If cond={!!wslCore?.dir}>
              <Button
                size="sm"
                variant="tertiary"
                className="h-7 shrink-0 rounded-md text-xs"
                onPress={onRevealDir}
              >
                {t('wsl_core.reveal_dir')}
              </Button>
            </If>
            <Button
              size="sm"
              variant="tertiary"
              className="h-7 shrink-0 rounded-md text-xs"
              isDisabled={busy || !selected || distros.length === 0}
              onPress={onImportKey}
            >
              {t('wsl_core.import_key')}
            </Button>
          </>
        )}
      />

      {confirmHolder}
      {installHolder}
    </div>
  )
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
      .then(() => {
        if (!cancelled)
          disclosure.confirm()
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
                    <PanelProgress logs={logs} />
                  </div>
                )}
              >
                <div className="flex flex-col items-start gap-3">
                  <div className="flex items-center gap-2">
                    <Spinner size="sm" color="current" />
                    <span className="text-xs text-muted">{t('wsl_core.installing_hint', { distro: props.distro })}</span>
                  </div>
                  <PanelProgress percentage={percentage} logs={logs} />
                </div>
              </If>
            </AlertDialog.Body>
            <AlertDialog.Footer>
              <If cond={error}>
                <Button className="rounded-md" variant="tertiary" onPress={disclosure.cancel}>
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
