import type { SCHEDULE_KINDS } from '../../shared/constants'

export type HostContext = any

export type ScheduleKind = (typeof SCHEDULE_KINDS)[number]

export type Weekday = 'MO' | 'TU' | 'WE' | 'TH' | 'FR' | 'SA' | 'SU'

export interface OnceSchedule {
  kind: 'once'
  at: string
  timeZone: string
}

export interface HourlySchedule {
  kind: 'hourly'
  minute: number
  timeZone: string
}

export interface DailySchedule {
  kind: 'daily'
  time: string
  timeZone: string
}

export interface IntervalSchedule {
  kind: 'interval'
  everyMinutes: number
  anchor?: string
  timeZone: string
}

export interface WorkdaysSchedule {
  kind: 'workdays'
  time: string
  timeZone: string
}

export interface WeeklySchedule {
  kind: 'weekly'
  weekdays: Weekday[]
  time: string
  timeZone: string
}

export interface MonthlySchedule {
  kind: 'monthly'
  day: number
  time: string
  timeZone: string
}

export interface CustomSchedule {
  kind: 'custom'
  everyDays: number
  anchor: string
  time: string
  timeZone: string
}

export type SchedulerSchedule = OnceSchedule | HourlySchedule | DailySchedule | IntervalSchedule | WorkdaysSchedule | WeeklySchedule | MonthlySchedule | CustomSchedule

export type RunTrigger = 'schedule' | 'manual'

export type RunStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'interrupted' | 'skipped' | 'cancelled'

export type SchedulerScheduleInput
  = | { kind: 'once', at: string, timeZone?: string }
    | { kind: 'hourly', minute: number, timeZone?: string }
    | { kind: 'daily', time: string, timeZone?: string }
    | { kind: 'interval', everyMinutes: number, anchor?: string, timeZone?: string }
    | { kind: 'workdays', time: string, timeZone?: string }
    | { kind: 'weekly', weekdays: readonly Weekday[], time: string, timeZone?: string }
    | { kind: 'monthly', day: number, time: string, timeZone?: string }
    | { kind: 'custom', everyDays: number, anchor?: string, time: string, timeZone?: string }

export interface SchedulerTask {
  id: string
  name: string
  schedule: SchedulerSchedule
  prompt: string
  recommendationId?: string
  workspaceId?: string
  permission?: string
  provider?: string
  model?: string
  reasoningEffort?: string
  module?: string
  agentPreset?: string
  enabled: boolean
  createdAt: string
  updatedAt: string
  lastRunAt?: string
  nextRunAt?: string
  /** 派生视图字段：已到点但受并发上限压住未启动；不落盘，读取路径每次重算。 */
  waiting?: boolean
}

export interface SchedulerRun {
  id: string
  taskId: string
  taskName: string
  trigger: RunTrigger
  status: RunStatus
  scheduledFor: string
  startedAt: string
  finishedAt?: string
  sessionId?: string
  error?: string
}

export interface TaskInput {
  name: string
  schedule: SchedulerScheduleInput
  prompt: string
  recommendationId?: string
  workspaceId?: string
  permission?: string
  provider?: string
  model?: string
  reasoningEffort?: string
  enabled?: boolean
}

export interface PermissionOption {
  value: string
  name: string
  description?: string
}

export interface ModelReasoningEffort {
  id: string
  name: string
  description?: string
}

export interface ModelReasoning {
  efforts: Array<ModelReasoningEffort>
  defaultEffort?: string
}

export interface ModelOption {
  provider: string
  providerLabel: string
  model: string
  label: string
  description?: string
  reasoning?: ModelReasoning
}

export interface ModelCatalogFailure {
  provider: string
  providerLabel: string
  message: string
}

export interface SchedulerOptions {
  workspaces: Array<{ id: string, path: string, title: string }>
  permissions: Array<PermissionOption>
  defaultPermission: string
  models: Array<ModelOption>
  failures: Array<ModelCatalogFailure>
  defaultModel: ModelOption | null
}

export type OperationResult<T extends object = object>
  = | ({ ok: true } & T)
    | { ok: false, error: string }
