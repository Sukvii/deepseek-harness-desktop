/*
 * @title dsh-tauri-scheduler
 * @swagger 2.0
 * @version 0.0.0
 */

import type { FetchOptions } from "dsh-tauri/client";
import { ofetch } from "dsh-tauri/client";
import type * as Types from "./index.type";

export const baseURL = "/api/desktop/dsh-tauri-scheduler";

/** @method get */
export function getHistory(params?: Types.GetHistoryQuery, options?: FetchOptions) {
  return ofetch<Types.RunListResponse>("/history", { baseURL, method: "get", params, ...options });
}

/** @method delete */
export function deleteHistory(body: Types.IdBody, options?: FetchOptions) {
  return ofetch<Types.ActionResult>("/history", { baseURL, method: "delete", body, ...options });
}

/** @method get */
export function getOptions(options?: FetchOptions) {
  return ofetch<Types.SchedulerOptions>("/options", { baseURL, method: "get", ...options });
}

/** @method post */
export function postRunsRecover(options?: FetchOptions) {
  return ofetch<Types.ActionResult>("/runs/recover", { baseURL, method: "post", ...options });
}

/** @method get */
export function getTasks(params?: Types.GetTasksQuery, options?: FetchOptions) {
  return ofetch<Types.TaskListResponse>("/tasks", { baseURL, method: "get", params, ...options });
}

/** @method put */
export function putTasks(body: Types.TaskUpdateBody, options?: FetchOptions) {
  return ofetch<Types.TaskActionResult>("/tasks", { baseURL, method: "put", body, ...options });
}

/** @method post */
export function postTasks(body: Types.TaskCreateBody, options?: FetchOptions) {
  return ofetch<Types.TaskActionResult>("/tasks", { baseURL, method: "post", body, ...options });
}

/** @method delete */
export function deleteTasks(body: Types.IdBody, options?: FetchOptions) {
  return ofetch<Types.ActionResult>("/tasks", { baseURL, method: "delete", body, ...options });
}

/** @method post */
export function postTasksRun(body: Types.IdBody, options?: FetchOptions) {
  return ofetch<Types.ActionResult>("/tasks/run", { baseURL, method: "post", body, ...options });
}

/** @method post */
export function postTasksToggle(body: Types.TaskToggleBody, options?: FetchOptions) {
  return ofetch<Types.TaskActionResult>("/tasks/toggle", { baseURL, method: "post", body, ...options });
}
