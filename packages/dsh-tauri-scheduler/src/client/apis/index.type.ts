export type ActionResult = {
  ok?: boolean;
  error?: string;
};
export type RunListResponse = {
  runs: SchedulerRun[];
};
export type SchedulerRun = {
  id: string;
  taskId: string;
  taskName: string;
  trigger: RunTrigger;
  status: RunStatus;
  scheduledFor: string;
  startedAt: string;
  finishedAt?: string;
  sessionId?: string;
  error?: string;
};
export type RunTrigger = "schedule" | "manual";
export type RunStatus = "queued" | "running" | "succeeded" | "failed" | "interrupted" | "skipped" | "cancelled";
export type SchedulerOptions = {
  workspaces: Array<{ id: string; path: string; title: string }>;
  permissions: Array<PermissionOption>;
  defaultPermission: string;
  models: Array<ModelOption>;
  failures: Array<ModelCatalogFailure>;
  defaultModel: ModelOption | null;
};
export type PermissionOption = {
  value: string;
  name: string;
  description?: string;
};
export type ModelOption = {
  provider: string;
  providerLabel: string;
  model: string;
  label: string;
  description?: string;
  reasoning?: ModelReasoning;
};
export type ModelReasoning = {
  efforts: Array<ModelReasoningEffort>;
  defaultEffort?: string;
};
export type ModelReasoningEffort = {
  id: string;
  name: string;
  description?: string;
};
export type ModelCatalogFailure = {
  provider: string;
  providerLabel: string;
  message: string;
};
export type TaskListResponse = {
  tasks: SchedulerTask[];
};
export type SchedulerTask = {
  id: string;
  name: string;
  schedule: SchedulerSchedule;
  prompt: string;
  recommendationId?: string;
  workspaceId?: string;
  permission?: string;
  provider?: string;
  model?: string;
  reasoningEffort?: string;
  module?: string;
  agentPreset?: string;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
  lastRunAt?: string;
  nextRunAt?: string;
  /** 派生视图字段：已到点但受并发上限压住未启动；不落盘，读取路径每次重算。 */
  waiting?: boolean;
};
export type SchedulerSchedule = OnceSchedule | HourlySchedule | DailySchedule | IntervalSchedule | WorkdaysSchedule | WeeklySchedule | MonthlySchedule | CustomSchedule;
export type OnceSchedule = {
  kind: "once";
  at: string;
  timeZone: string;
};
export type HourlySchedule = {
  kind: "hourly";
  minute: number;
  timeZone: string;
};
export type DailySchedule = {
  kind: "daily";
  time: string;
  timeZone: string;
};
export type IntervalSchedule = {
  kind: "interval";
  everyMinutes: number;
  anchor?: string;
  timeZone: string;
};
export type WorkdaysSchedule = {
  kind: "workdays";
  time: string;
  timeZone: string;
};
export type WeeklySchedule = {
  kind: "weekly";
  weekdays: Weekday[];
  time: string;
  timeZone: string;
};
export type Weekday = "MO" | "TU" | "WE" | "TH" | "FR" | "SA" | "SU";
export type MonthlySchedule = {
  kind: "monthly";
  day: number;
  time: string;
  timeZone: string;
};
export type CustomSchedule = {
  kind: "custom";
  everyDays: number;
  anchor: string;
  time: string;
  timeZone: string;
};
export type SchedulerScheduleInput = { kind: "once"; at: string; timeZone?: string } | { kind: "hourly"; minute: number; timeZone?: string } | { kind: "daily"; time: string; timeZone?: string } | { kind: "interval"; everyMinutes: number; anchor?: string; timeZone?: string } | { kind: "workdays"; time: string; timeZone?: string } | { kind: "weekly"; weekdays: readonly Weekday[]; time: string; timeZone?: string } | { kind: "monthly"; day: number; time: string; timeZone?: string } | { kind: "custom"; everyDays: number; anchor?: string; time: string; timeZone?: string };
export type TaskActionResult = {
  ok?: boolean;
  task?: SchedulerTask;
  error?: string;
};

export interface IdBody {
  id: string;
}
export interface TaskCreateBody {
  name: string;
  schedule: SchedulerScheduleInput;
  prompt: string;
  recommendationId?: string;
  workspaceId?: string;
  permission?: string;
  provider?: string;
  model?: string;
  reasoningEffort?: string;
  enabled?: boolean;
}
export interface TaskUpdateBody {
  id: string;
  name?: string;
  schedule?: SchedulerScheduleInput;
  prompt?: string;
  recommendationId?: string;
  workspaceId?: string;
  permission?: string;
  provider?: string;
  model?: string;
  reasoningEffort?: string;
  enabled?: boolean;
}
export interface TaskToggleBody {
  id: string;
  enabled: boolean;
}
export interface GetHistoryQuery {
  taskId?: string;
}
export interface GetTasksQuery {
  search?: string;
}
