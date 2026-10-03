import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { isWslDataDirProbePending, WSL_DATA_DIR_PROBE_PENDING } from '@/utils/wsl-runtime-info'

/**
 * R-U9-2 / R-U9-3 的前端回归。
 *
 * 两层判据：
 * 1. **纯逻辑**：未探测标记的识别，以及「打开数据目录」按钮的禁用条件
 *    （未探测 / 空路径都必须禁用，不能让按钮去开一个说不清的目录）。
 * 2. **接线守卫**：诊断面板必须把 `data_dir` 交给后端、且在未探测时禁用按钮；
 *    后端标记字符串必须与前端常量逐字一致（两侧分开定义，改一处要改另一处）。
 *    这一层用源码文本断言——仓库既有同类守卫（`test/core-local-unsupported.test.ts`）。
 */

const read = (rel: string) => readFileSync(new URL(`../${rel}`, import.meta.url), 'utf8')

/** 从 `debug.tsx` 取「打开数据目录」按钮所在片段，用于断言禁用条件已接线 */
function revealButtonSource(): string {
  const source = read('src/ui/config/debug.tsx')
  const start = source.indexOf('const revealDisabled')
  const end = source.indexOf('onRevealDataDir()')
  expect(start, 'debug.tsx 缺少 revealDisabled 计算').toBeGreaterThan(-1)
  expect(end, 'debug.tsx 缺少 onRevealDataDir 调用').toBeGreaterThan(start)
  return source.slice(start, end)
}

describe('wsl 未探测标记（R-U9-2）', () => {
  it('逐字匹配后端标记', () => {
    expect(isWslDataDirProbePending(WSL_DATA_DIR_PROBE_PENDING)).toBe(true)
  })

  it('前后空白仍识别为标记（后端可能带换行）', () => {
    expect(isWslDataDirProbePending(` ${WSL_DATA_DIR_PROBE_PENDING}\n`)).toBe(true)
  })

  it('真实路径与缺失值都不算未探测', () => {
    expect(isWslDataDirProbePending('\\\\wsl.localhost\\Ubuntu\\home\\u\\.dsh-desktop')).toBe(false)
    expect(isWslDataDirProbePending('C:\\Users\\u\\AppData\\Roaming\\dsh-tauri')).toBe(false)
    expect(isWslDataDirProbePending('')).toBe(false)
    expect(isWslDataDirProbePending(undefined)).toBe(false)
    expect(isWslDataDirProbePending(null)).toBe(false)
  })
})

describe('「打开数据目录」按钮的禁用条件（R-U9-3）', () => {
  /** 与 debug.tsx 中 `revealDisabled` 同一判据 */
  const disabled = (dataDir: string | undefined) => !dataDir || isWslDataDirProbePending(dataDir)

  it('未探测时禁用（不得回落去开 Windows 目录）', () => {
    expect(disabled(WSL_DATA_DIR_PROBE_PENDING)).toBe(true)
  })

  it('空路径禁用', () => {
    expect(disabled('')).toBe(true)
    expect(disabled(undefined)).toBe(true)
  })

  it('有效路径（含 WSL UNC）可用', () => {
    expect(disabled('\\\\wsl.localhost\\Ubuntu\\home\\u\\.dsh-desktop')).toBe(false)
    expect(disabled('C:\\Users\\u\\AppData\\Roaming\\dsh-tauri')).toBe(false)
  })
})

describe('诊断面板接线守卫（R-U9-2 / R-U9-3）', () => {
  it('按钮把界面上显示的数据目录原样交给后端', () => {
    const source = read('src/ui/config/debug.tsx')
    expect(source).toContain('invoke(\'reveal_data_dir\', { path: info?.data_dir ?? \'\' })')
  })

  it('按钮在未探测/空路径时禁用', () => {
    const button = revealButtonSource()
    expect(button).toContain('const revealDisabled = !info?.data_dir || dataDirUnprobed')
    expect(button).toContain('isDisabled={revealDisabled}')
  })

  it('未探测时渲染「未探测」提示，而不是把标记当路径显示', () => {
    const source = read('src/ui/config/debug.tsx')
    expect(source).toContain('const dataDirUnprobed = isWslDataDirProbePending(info?.data_dir)')
    expect(source).toContain('t(\'ui.wsl_linux_facts_pending\')')
  })
})

describe('前端常量与后端标记一致（R-U9-2）', () => {
  it('system_os.rs 的 DATA_DIR_PROBE_PENDING 与前端同值', () => {
    const source = read('src-tauri/src/bridge/system_os.rs')
    const match = source.match(/const DATA_DIR_PROBE_PENDING: &str = "([^"]+)"/)
    expect(match, 'system_os.rs 缺少 DATA_DIR_PROBE_PENDING 常量').not.toBeNull()
    expect(match![1]).toBe(WSL_DATA_DIR_PROBE_PENDING)
  })

  it('wsl 分支不用宿主字段兜底（node_version 先清空、data_dir 先置标记）', () => {
    const source = read('src-tauri/src/bridge/system_os.rs')
    const start = source.indexOf('fn apply_wsl_runtime_info')
    const end = source.indexOf('/// probe 缓存里与本函数相关的字段')
    expect(start).toBeGreaterThan(-1)
    expect(end).toBeGreaterThan(start)
    const body = source.slice(start, end)
    expect(body).toContain('info.node_version = String::new()')
    expect(body).toContain('info.data_dir = DATA_DIR_PROBE_PENDING.to_string()')
    // 不得出现 Windows 宿主来源兜底（`config::get_dsh_data_path` 是宿主数据根的唯一来源）
    expect(body).not.toContain('.or(')
    expect(body).not.toContain('get_dsh_data_path')
    // 「不回落宿主值」的行为判据由 Rust 单测锁定：
    // bridge::system_os::tests::wsl_runtime_info_without_probe_clears_host_fields
  })
})
