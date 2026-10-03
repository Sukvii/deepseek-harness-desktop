/** Rust 侧 `config::RuntimeInfo` 的序列化形态（snake_case） */
export interface RuntimeInfo {
  app_version: string
  /** 当前活动核心的版本号（本地读取，界面展示用） */
  dsh_version: string | null
  node_version: string
  service_url: string
  data_dir: string
  log_path: string
  platform: string
  arch: string
  /**
   * 后端判定的实际生效核心来源（`local` / `app` / `wsl`）。
   *
   * 判断「谁在跑」时以此为准，不要用 store 的 `active_core` 拼状态组合
   * （R-U9-4）。空串 = 旧后端或未经 bridge 的构造，按「未知」处理。
   */
  active_source: 'local' | 'app' | 'wsl' | ''
}
