import type { HarnessCore } from '@/types'
import { describe, expect, it } from 'vitest'
import { compareCores, crossesWslBoundary, isUnsupportedCore, SOURCE_RANK } from '@/ui/config/core-list'
import { isCoreUnsupported, MIN_SUPPORTED_CORE_VERSION } from '@/utils/core-version'

/** 列表夹具：只写出该用例关心的字段，其余按「可用、未激活、可卸载」的常规行补齐。 */
function core(overrides: Partial<HarnessCore>): HarnessCore {
  return {
    id: 'app-0.1.5-rc.3',
    source: 'app',
    version: '0.1.5-rc.3',
    tag: 'dsh-0.1.5-rc.3-1',
    path: '',
    dir: '',
    present: true,
    active: false,
    removable: true,
    preview: false,
    aboveRecommended: false,
    orphaned: false,
    bundled: false,
    recommendedVersion: null,
    ...overrides,
  }
}

describe('core list ordering', () => {
  it('来源展示顺序为 local → wsl → app 且比较器自洽', () => {
    const rows = [
      core({ id: 'app', source: 'app' }),
      core({ id: 'wsl', source: 'wsl', version: '0.1.2-rc.1' }),
      core({ id: 'local', source: 'local', version: '0.1.5-rc.1' }),
    ]

    // 输入次序刻意打乱：来源顺序必须由比较器决定，而非沿用后端返回顺序
    expect([...rows].reverse().sort(compareCores).map(r => r.id)).toEqual(['local', 'wsl', 'app'])
    expect([rows[1], rows[2], rows[0]].sort(compareCores).map(r => r.id)).toEqual(['local', 'wsl', 'app'])

    // W1 审核 R-2：旧比较器对 wsl 不满足全序（sgn(a,b) 与 sgn(b,a) 不互反），
    // 这里把「同源相等、跨源反对称」钉成契约，退化成非全序时必红。
    // 注意 `Math.sign` 对同源会比较出 `-0`，而 `Object.is(-0, 0)` 为 false，
    // 故先把符号归一到整数域再取反，避免用 `-0` 与 `+0` 对比出假红。
    const sign = (n: number) => Math.sign(n) || 0
    for (const x of rows) {
      for (const y of rows) {
        expect(sign(compareCores(x, y))).toBe(sign(-compareCores(y, x)))
      }
    }
  })

  it('来源序号全序 local < wsl < app，且不随版本高低改变', () => {
    expect(SOURCE_RANK.local).toBeLessThan(SOURCE_RANK.wsl)
    expect(SOURCE_RANK.wsl).toBeLessThan(SOURCE_RANK.app)

    for (const [a, b] of [
      ['local', 'wsl'],
      ['wsl', 'app'],
      ['local', 'app'],
    ] as const) {
      const backwards = core({ id: 'backwards', source: a, version: '0.1.0-alpha.1' })
      const forwards = core({ id: 'forwards', source: b, version: '9.9.9' })
      expect(compareCores(backwards, forwards)).toBeLessThan(0)
      expect(compareCores(forwards, backwards)).toBeGreaterThan(0)
    }
  })

  it('随包内核置顶，与来源及版本无关', () => {
    const bundled = core({ id: 'bundled', source: 'app', version: '0.1.5-rc.1', bundled: true })
    const local = core({ id: 'local', source: 'local', version: '0.2.0' })
    const wsl = core({ id: 'wsl', source: 'wsl', version: '0.2.0' })

    expect([local, bundled, wsl].sort(compareCores)[0]!.id).toBe('bundled')
    expect([local, wsl, bundled].sort(compareCores).map(r => r.id)).toEqual(['bundled', 'local', 'wsl'])
  })

  it('预打包核心按 SemVer 降序，版本缺失或带前缀时回落到 tag', () => {
    const rows = [
      core({ id: 'app-old', source: 'app', version: '0.1.5-rc.1', tag: 'dsh-0.1.5-rc.1-1' }),
      core({ id: 'app-new', source: 'app', version: '0.2.0-rc.2', tag: 'dsh-0.2.0-rc.2-2' }),
      core({ id: 'app-mid', source: 'app', version: '0.1.7-alpha.1', tag: 'dsh-0.1.7-alpha.1-3' }),
    ]

    expect(rows.sort(compareCores).map(r => r.id)).toEqual(['app-new', 'app-mid', 'app-old'])

    // 后端可能从历史 package.json 拿到带 src- 前缀的版本：前缀必须先剥掉，
    // 否则「以 tag 兜底」这一条会把当前激活版本排到末尾。
    const messy = core({ id: 'messy', source: 'app', version: '', tag: 'src-0.1.7-alpha.1-99' })
    const clean = core({ id: 'clean', source: 'app', version: '', tag: 'dsh-0.1.6-rc.2-7' })
    expect([clean, messy].sort(compareCores).map(r => r.id)).toEqual(['messy', 'clean'])
  })
})

describe('core list compatibility predicate', () => {
  it('非 WSL 来源沿用最低支持基线判定', () => {
    expect(isCoreUnsupported(MIN_SUPPORTED_CORE_VERSION)).toBe(false)

    expect(isUnsupportedCore(core({ source: 'local', version: '0.1.5-rc.3' }))).toBe(false)
    expect(isUnsupportedCore(core({ source: 'local', version: '0.1.5-rc.1' }))).toBe(false)
    expect(isUnsupportedCore(core({ source: 'local', version: '0.1.5-alpha.2' }))).toBe(true)
    expect(isUnsupportedCore(core({ source: 'app', version: '0.1.2-rc.1' }))).toBe(true)
  })

  it('版本号缺失时用 release tag 判定', () => {
    expect(isUnsupportedCore(core({ source: 'app', version: '', tag: 'dsh-0.1.4-rc.1-42' }))).toBe(true)
    expect(isUnsupportedCore(core({ source: 'app', version: '', tag: 'dsh-0.1.5-rc.3-42' }))).toBe(false)
  })

  it('受控运行时决定 WSL 行的兼容基线，不受本机最低支持基线约束（U6.5①）', () => {
    // WSL 核心的兼容基线由受控 runtime 的独立推荐值决定，硬套本机基线会把可用行
    // 收进默认折叠的「不兼容版本」分组 —— 去掉 WSL 例外时本条必须变红。
    for (const version of ['0.1.0', '0.0.9', MIN_SUPPORTED_CORE_VERSION, '0.2.0-rc.2', 'relative']) {
      expect(isUnsupportedCore(core({ source: 'wsl', version, tag: `dsh-${version}-1` }))).toBe(false)
    }
    expect(isUnsupportedCore(core({ source: 'wsl', version: '', tag: 'dsh-0.1.0-1' }))).toBe(false)
  })
})

describe('core list WSL boundary', () => {
  it('目标核心是 WSL 时视为跨边界', () => {
    const wsl = core({ id: 'wsl', source: 'wsl' })
    expect(crossesWslBoundary(wsl, undefined)).toBe(true)
    expect(crossesWslBoundary(wsl, core({ id: 'local', source: 'local' }))).toBe(true)
    expect(crossesWslBoundary(wsl, wsl)).toBe(true)
  })

  it('在用核心是 WSL 时同样视为跨边界（含无激活行）', () => {
    const active = core({ id: 'active-wsl', source: 'wsl', active: true })
    expect(crossesWslBoundary(core({ id: 'local', source: 'local' }), active)).toBe(true)
    expect(crossesWslBoundary(core({ id: 'app', source: 'app' }), active)).toBe(true)
  })

  it('两侧都是本机核心时不跨边界', () => {
    // 只有两侧都不是 WSL 时才继续套用本机升级档案守卫（U6.5③）。
    expect(crossesWslBoundary(core({ id: 'app', source: 'app' }), undefined)).toBe(false)
    expect(crossesWslBoundary(
      core({ id: 'app', source: 'app' }),
      core({ id: 'local', source: 'local', active: true }),
    )).toBe(false)
  })
})
