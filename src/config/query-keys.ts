/**
 * react-query 查询键集中定义。
 *
 * 查询键是「谁在读写同一份缓存」的契约：面板查询、store 的重启失效、后端事件
 * 写入缓存都按同一份字面量操作，散落各处会出现「失效不生效」的静默 bug。
 */
export const queryKeys = {
  taskManager: ['task_manager'] as const,
  taskManagerLogs: ['task_manager_logs'] as const,
  /** 运行时信息（版本 / 端口 / 路径，debug 面板） */
  info: ['info'] as const,
  /** CLI 链接状态（debug 面板） */
  cliStatus: ['cli_status'] as const,
  /** 开机自启开关 */
  launchOnLogin: ['launch_on_login'] as const,
  /** Harness 核心列表（local / wsl / app-<tag>） */
  cores: ['cores'] as const,
  /** 已安装的 WSL 发行版列表（WSL 面板） */
  wslDistros: ['wsl_distros'] as const,
  /** 指定发行版的 WSL 核心探测结果（键按发行版隔离，切换目标即重新探测） */
  wslProbe: (distro: string) => ['wsl_probe', distro] as const,
  /** WSL 核心独立推荐版本（清单 `engines.dsh.wslRecommend`；安装确认框用） */
  wslRecommendedVersion: ['wsl_recommended_version'] as const,
  /** 已安装 dsh 插件列表（面板 / 配置对话框角标 / 导航栏共用） */
  plugins: ['plugins'] as const,
  /** dsh 档案列表 */
  profiles: ['profiles'] as const,
  /** 当前档案的备份快照列表 */
  backups: ['backups'] as const,
  remoteMachines: ['remote_machines'] as const,
  remoteEvents: ['remote_events'] as const,
} as const
