import type { CoreSource, HarnessCore } from '@/types'
import { compareVersions, isCoreUnsupported } from '@/utils/core-version'

/**
 * 「核心」面板的纯列表/身份逻辑（U8.3 T6/T7：自 `core.tsx` 抽出以便单测）。
 *
 * 注意：上游 `test/core-local-unsupported.test.ts` 按源码扫描 `core.tsx` 中
 * `isUnsupportedLocal` 的函数体（并要求组件的「不兼容」提示引用
 * `MIN_SUPPORTED_CORE_VERSION`），因此该谓词与提示文案保留在 `core.tsx`；
 * 本模块只承载可独立断言的判定与排序。
 */

/** 核心列表的来源展示顺序（全序；W1 审核 R-2：旧比较器对 wsl 不满足全序） */
export const SOURCE_RANK: Record<CoreSource, number> = { local: 0, wsl: 1, app: 2 }

/**
 * 列表排序比较器：随包内核（离线包）固定置顶，其后按来源全序 local → wsl → app，
 * 预打包核心内部按 SemVer 降序（后端可能从历史 package.json 得到带 src-/dsh-src-
 * 前缀的版本，排序时使用 tag 作为兜底，避免当前激活版本被排到末尾）。
 */
export function compareCores(a: HarnessCore, b: HarnessCore): number {
  if (a.bundled !== b.bundled)
    return a.bundled ? -1 : 1
  if (a.source !== b.source)
    return SOURCE_RANK[a.source] - SOURCE_RANK[b.source]
  if (a.source !== 'app')
    return 0
  return -compareVersions(a.version || a.tag, b.version || b.tag)
}

/**
 * 跨 Windows/WSL 切换（U6.5③）：档案与插件都是 Windows 数据，跨环境切换不创建
 * Windows 版本档案，因此不套用本机升级档案守卫（走普通切换确认）。
 */
export function crossesWslBoundary(target: HarnessCore, active: HarnessCore | undefined): boolean {
  return target.source === 'wsl' || active?.source === 'wsl'
}

/**
 * 核心是否低于最低支持基线。低于基线的旧版本无法加载随包内置插件（issue #596），
 * 核心列表把它们收进默认折叠的「不兼容版本」分组。
 *
 * WSL 行不套用本机基线（U6.5①）：该基线与内置插件相关，只约束本机 Local/App；
 * WSL 核心的兼容基线由受控 runtime 的独立推荐值决定。
 */
export function isUnsupportedCore(core: HarnessCore): boolean {
  return core.source !== 'wsl' && isCoreUnsupported(core.version || core.tag)
}
