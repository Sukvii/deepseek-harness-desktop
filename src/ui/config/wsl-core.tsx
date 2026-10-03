import type { HarnessCore, WslCoreProbe, WslDistro } from '@/types'
import type { WslCoreInstallDialogProps } from '@/ui/dialog/wsl-core-install'
import { Button, Chip, Description, Label, ListBox, Select, Spinner } from '@heroui/react'
import { useOverlay } from '@overlastic/react'
import { useWatch } from '@reause/core'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { If } from 'react-if-lite'
import { useStore } from 'valtio-define'
import { Item } from '@/components/item'
import { Modal } from '@/components/modal'
import { Panel } from '@/components/panel'
import { queryKeys } from '@/config/query-keys'
import { useInvalidateOnSettingUpdated } from '@/hooks/use-invalidate-on-setting-updated'
import { store } from '@/store'
import { WslCoreInstallDialog } from '@/ui/dialog/wsl-core-install'
import { silence } from '@/utils/silence'
import { toast } from '@/utils/toast'

/**
 * 「WSL 核心」面板（方案 W5；R-W5-2/3/4 返修；U6 迁移到声明式查询）。
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
 * 读取与探测走声明式查询（U6.2）：发行版列表 / 按发行版隔离的探测 / WSL 独立
 * 推荐版本各有查询键；只有「手动检测」与安装结果回写缓存，不在查询成功时改写
 * 设置。持久化统一 `store.setting.update({ wslDistro })`。
 */
export function ConfigWslCore() {
  const { t } = useTranslation()
  const { serviceRunning } = useStore(store.harness)
  const { wsl_distro } = useStore(store.setting)

  const [confirmHolder, openConfirm] = useOverlay(Modal, { type: 'holder' })
  const [installHolder, openInstall] = useOverlay<WslCoreInstallDialogProps, WslCoreProbe>(WslCoreInstallDialog, { type: 'holder' })

  const [busy, setBusy] = useState(false)

  /** 已确认（持久化）的发行版 */
  const distro = wsl_distro ?? ''
  const queryClient = useQueryClient()
  const { data: cores = [], refetch: refreshCores } = useQuery({
    queryKey: queryKeys.cores,
    queryFn: () => invoke<HarnessCore[]>('get_cores'),
  })
  useInvalidateOnSettingUpdated(queryKeys.cores)
  const distroQuery = useQuery({
    queryKey: queryKeys.wslDistros,
    queryFn: () => invoke<WslDistro[]>('list_wsl_distros'),
  })
  const probeQuery = useQuery({
    queryKey: queryKeys.wslProbe(distro),
    queryFn: () => invoke<WslCoreProbe>('probe_wsl_core', { distro }),
    enabled: distro.length > 0,
  })
  const recommendation = useQuery({
    queryKey: queryKeys.wslRecommendedVersion,
    queryFn: () => invoke<string>('get_wsl_recommended_version'),
  })
  // 探测完成后刷新核心列表：WSL 行（`dir` / `version` / `present`）取自后端探测
  // 缓存，否则首次选定发行版时该行尚无缓存，「数据目录」会一直停在占位符。
  useWatch(probeQuery.data, (value) => {
    if (value)
      void queryClient.invalidateQueries({ queryKey: queryKeys.cores })
  })

  const distros = distroQuery.data ?? []
  /** 下拉显示候选：默认项回落首项；只作显示，不写设置（R-W5-3） */
  const candidate = distros.find(item => item.isDefault)?.name ?? distros[0]?.name ?? ''
  /** 下拉显示值：已确认优先，否则候选 */
  const selected = distro || candidate
  /** 只认与当前显示目标一致的探测结果，避免切换后展示上一个发行版的数据 */
  const current = probeQuery.data?.distro === selected ? probeQuery.data : null
  const wslCore = cores.find(core => core.source === 'wsl')
  const active = wslCore?.active ?? false
  /**
   * 运行中限制只针对「WSL 来源」正在运行（R-W5-2）：`serviceRunning` 不区分来源，
   * Windows 核心运行时必须仍可选择 / 探测 / 安装 WSL，不打断当前会话。
   */
  const wslRunning = serviceRunning && active
  const probing = probeQuery.isFetching
  const ready = !!current?.dsh && current.skipAuthReady
  const installed = !!current?.dsh
  const nodeMissing = !!current && !current.node
  const patchMissing = !!current?.dsh && !current.skipAuthReady

  /** 持久化发行版为已确认配置（WSL 运行中会被后端 `WSL_DISTRO_LOCKED` 拒绝） */
  async function persistDistro(name: string) {
    try {
      await store.setting.update({ wslDistro: name })
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

  /** 「重新检测」：已确认目标现场重探；候选先确认，再由按 distro 隔离的查询探测 */
  async function onProbeSelected() {
    const target = selected
    if (!target || busy || probing)
      return
    if (target === distro) {
      await probeQuery.refetch()
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
    // 推荐兼容基线取 U1 的 WSL 独立推荐值（清单 `engines.dsh.wslRecommend`），
    // 不展示桌面 recommend 当 WSL 目标。
    const recommended = recommendation.data ?? ''
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
      // 先停服再进进度对话框：否则前端 shutdown 会把进度面板一起关掉（W5.3）。
      // 停止失败直接进入恢复分支，不继续安装（U6.6）。
      if (wasRunning)
        await invoke('shutdown_harness')
      const probe = await openInstall({
        distro: target,
        version: current?.dshVersion ?? undefined,
        runInstall: () => invoke<WslCoreProbe>('install_wsl_core', { distro: target }),
      })
      // 安装返回最新探测：写入按目标隔离的查询键，再刷新核心列表（U6.2）
      queryClient.setQueryData(queryKeys.wslProbe(target), probe)
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
      // 完整 restart 会按既有行为关闭设置面板；面板仍在时刷新核心列表
      await refreshCores()
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
      <Panel.Header title={t('wsl_core.title')} description={t('wsl_core.tooltip')} />

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
            <Select.Trigger className="min-h-8! h-8 py-0 items-center">
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
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
            className="h-8 shrink-0 text-xs"
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
        <If cond={distroQuery.isError && distros.length === 0}>
          <Description className="text-xs text-danger">{t('wsl_core.distros_failed')}</Description>
        </If>
        <If cond={!distroQuery.isError && distroQuery.isSuccess && distros.length === 0}>
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
            {/* 探测失败时不展示「未安装」：错误不能被伪装成未安装状态 */}
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
        <If cond={probeQuery.isError}>
          <Description className="break-all font-mono text-xs text-danger">
            {String(probeQuery.error)}
          </Description>
        </If>

        {/* 操作 */}
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            className="h-8 text-xs"
            isDisabled={busy || probing || !selected || nodeMissing || distros.length === 0}
            onPress={onInstall}
          >
            {installed ? t('wsl_core.update') : t('wsl_core.install')}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            className="h-8 text-xs"
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
                className="h-7 shrink-0 text-xs"
                onPress={onRevealDir}
              >
                {t('wsl_core.reveal_dir')}
              </Button>
            </If>
            <Button
              size="sm"
              variant="tertiary"
              className="h-7 shrink-0 text-xs"
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
