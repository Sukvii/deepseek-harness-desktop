import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { toActiveCoreSource } from '@/ui/config/hooks/use-active-core-source'

/**
 * R-U9-4 的回归。
 *
 * 两层判据：
 * 1. **纯逻辑**：后端 `active_source` 字符串 → 前端判定的收敛（`''`/未知值按「尚未取到」）。
 *    后端四类状态（有效 WSL / 清除发行版 / 非 Windows / 本机选择）本身由 Rust 单测
 *    `service::core::source::tests::explicit_wsl_selection_requires_platform_flag_and_distro`
 *    与 `resolve_local_or_app_follows_upstream_rules` 锁定，前端只消费其结论。
 * 2. **接线守卫**：两个面板必须共用同一个后端来源 hook，不得再各自从 store 拼
 *    `active_core` 状态——那正是「清除发行版后 active_core 仍是 wsl」导致面板显示错的根因。
 */

const read = (rel: string) => readFileSync(new URL(`../${rel}`, import.meta.url), 'utf8')

/**
 * 去掉注释再断言「旧判定形态不再出现」。
 * 保留注释会让「解释为什么不这么做」的说明文字被误判成旧代码（而且它本身有价值）。
 */
function stripComments(source: string) {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '')
}

const readCode = (rel: string) => stripComments(read(rel))

const WSL_PANELS = [
  'src/ui/config/plugin.tsx',
  'src/ui/config/profile.tsx',
] as const

describe('active_source 收敛（R-U9-4）', () => {
  it('三种来源原样通过', () => {
    expect(toActiveCoreSource('local')).toBe('local')
    expect(toActiveCoreSource('app')).toBe('app')
    expect(toActiveCoreSource('wsl')).toBe('wsl')
  })

  it('空串与未知值按「尚未取到」处理', () => {
    expect(toActiveCoreSource('')).toBeUndefined()
    expect(toActiveCoreSource(undefined)).toBeUndefined()
    expect(toActiveCoreSource('WSL')).toBeUndefined()
    expect(toActiveCoreSource('bundled')).toBeUndefined()
  })

  it('wsl 判定只在来源确为 wsl 时为真', () => {
    const isWsl = (value: string | undefined) => toActiveCoreSource(value) === 'wsl'
    expect(isWsl('wsl')).toBe(true)
    expect(isWsl('local')).toBe(false)
    expect(isWsl('app')).toBe(false)
    // 关键：后端在「清空发行版」后会给出 local/app，此时面板必须按 Windows 内容渲染
    expect(isWsl(undefined)).toBe(false)
  })
})

describe('面板接线守卫（R-U9-4）', () => {
  for (const panel of WSL_PANELS) {
    it(`${panel} 使用后端来源 hook，而不是自行比对 active_core`, () => {
      const code = readCode(panel)
      expect(code).toContain('useWslCoreActive')
      expect(code).toContain('@/ui/config/hooks/use-active-core-source')
      // 旧判定形态必须从**代码**里消失：既不能读 active_core，也不能再引入 valtio 的 useStore
      expect(code).not.toContain('active_core')
      expect(code).not.toContain('useStore')
    })
  }

  it('hook 消费后端的 active_source，且不做联网查询', () => {
    const code = readCode('src/ui/config/hooks/use-active-core-source.ts')
    expect(code).toContain('invoke<RuntimeInfo>(\'get_runtime_info\')')
    expect(code).toContain('data?.active_source')
    // 判来源不得引入 releases 之类的网络查询
    expect(code).not.toContain('fetch(')
    expect(code).not.toContain('releases')
  })

  it('后端在 bridge 层把 active_source 写进运行时信息', () => {
    const source = read('src-tauri/src/bridge/system_os.rs')
    expect(source).toContain('info.active_source = core::active_source(&app_handle).as_str().to_string()')
  })

  it('runtimeInfo 契约在前端类型里有 active_source 字段', () => {
    const source = read('src/types/runtime.ts')
    expect(source).toContain('active_source')
    expect(source).toContain('\'local\' | \'app\' | \'wsl\' | \'\'')
  })
})
