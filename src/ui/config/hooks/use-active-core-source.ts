import type { RuntimeInfo } from '@/types'
import { useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { queryKeys } from '@/config/query-keys'
import { useInvalidateOnSettingUpdated } from '@/hooks/use-invalidate-on-setting-updated'

/** 后端判定的实际生效核心来源（`CoreSource::as_str()` 的三种取值） */
export type ActiveCoreSource = 'local' | 'app' | 'wsl'

/** 把后端的来源字符串收敛为本类型；空串（旧后端/未经 bridge 的构造）按「尚未取到」处理 */
export function toActiveCoreSource(value: string | undefined): ActiveCoreSource | undefined {
  return value === 'local' || value === 'app' || value === 'wsl' ? value : undefined
}

/**
 * 「当前谁在跑」的权威来源（R-U9-4）。
 *
 * 面板此前各自比对 store 里的 `active_core === 'wsl'`，但后端 `active_source`
 * 还要求平台支持且发行版非空白：`update_app_config` 清空 `wsl_distro` 时**不会**
 * 同时清 `active_core`，于是 `{active_core: 'wsl', wsl_distro: null}` 是现有契约
 * 能产生的状态——后端已按本机来源工作，面板却仍显示 WSL 说明并隐藏 Windows 内容。
 *
 * 这里直接消费 `get_runtime_info` 的 `active_source`（纯本地读取，不联网拉
 * releases），与后端判定保持同一口径；设置变更（含清除发行版）后重新取值。
 *
 * @returns 生效来源；`undefined` = 尚未取到（调用方保守处理，例如沿用旧判定）
 */
export function useActiveCoreSource(): ActiveCoreSource | undefined {
  useInvalidateOnSettingUpdated(queryKeys.info)
  const { data } = useQuery({
    queryKey: queryKeys.info,
    queryFn: () => invoke<RuntimeInfo>('get_runtime_info'),
  })
  return toActiveCoreSource(data?.active_source)
}

/** 生效来源是否为 WSL 核心（`undefined` 时返回 false，UI 不显示 WSL 专属说明） */
export function useWslCoreActive(): boolean {
  return useActiveCoreSource() === 'wsl'
}
