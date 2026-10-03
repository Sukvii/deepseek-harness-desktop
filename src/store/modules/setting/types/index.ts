import type { Appearance } from '../../../../../packages/dsh-tauri/src/shared/appearance'

export interface AppSettingUpdate {
  appearance?: Appearance
  port?: number
  zoomFactor?: number
  harnessMaxHeapMb?: number
  autoStart?: boolean
  cliLinkEnabled?: boolean
  closeAction?: string
  backupRetentionCount?: number
  backupIncludeCredentials?: boolean
  /** 选定的 WSL 发行版（空串 = 清除为未选择；W 系列方案 W5） */
  wslDistro?: string
}

export type ZoomAction = 'increase' | 'decrease' | 'reset'
