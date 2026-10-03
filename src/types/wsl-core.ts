/** Rust 侧 `service::wsl_core::exec::WslDistro` 的序列化形态 */
export interface WslDistro {
  name: string
  state: string
  version: string
  isDefault: boolean
}

/** Rust 侧 `service::wsl_core::probe::WslCoreProbe` 的序列化形态 */
export interface WslCoreProbe {
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
