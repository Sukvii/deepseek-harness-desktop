import { defineConfig } from 'vitest/config'
import { SHARED_ALIAS } from './tooling.config'

/**
 * `wsl-real` 入口：WSL 核心真机用例（**必须显式选择**）。
 *
 * 为什么不并入 `vitest.desktop.config.ts` 的桌面 project（D-U8-13 裁决）：
 * 真机用例要求「发行版 `Ubuntu` 的默认用户是隔离用户、且该用户家目录里已有 U8.4 受控安装
 * 产物」这套**只有专用环境才具备**的前置。普通 CI runner（`windows-latest`）没有发行版，
 * 把它留在桌面车道里只会让整个 workflow 变红；而自动 skip 又会把「前置缺失」伪装成
 * 「跳过通过」（仓库 `docs/specs/testing.md:168-177` 明确禁止）。
 *
 * 于是拆成两个入口：
 *   - 普通桌面/CI：`node node_modules/vitest/vitest.mjs run --project desktop`
 *     （收集 `test/e2e/desktop/*.e2e.ts`，不再包含真机文件）；
 *   - 真机（已获授权的隔离环境）：`node node_modules/vitest/vitest.mjs run --config vitest.wsl-real.config.ts`
 *     —— 前置不满足时**直接失败**，不 skip、不自动改环境。
 *
 * 不把本配置加入 `vitest.config.ts` 的默认 `projects`：裸跑全项目时不应隐式启动真机车道。
 * 其余设置（`fileParallelism: false`、隔离 HOME/store、debug 二进制与 WebDriver 准备）沿用
 * 桌面 project 的既有约定——应用固定占用 debug 端口，一次只允许一个实例。
 */
export default defineConfig({
  resolve: { alias: SHARED_ALIAS },
  test: {
    name: 'wsl-real',
    include: ['test/e2e/wsl-real/*.e2e.ts'],
    globalSetup: ['./test/e2e/setup-desktop.ts'],
    environment: 'node',
    fileParallelism: false,
    testTimeout: 180_000,
    hookTimeout: 180_000,
  },
})
