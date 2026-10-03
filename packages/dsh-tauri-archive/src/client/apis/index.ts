/*
 * @title dsh-tauri-archive
 * @swagger 2.0
 * @version 0.0.0
 */

import type { FetchOptions } from "dsh-tauri/client";
import { ofetch } from "dsh-tauri/client";
import type * as Types from "./index.type";

export const baseURL = "/api/desktop/dsh-tauri-archive";

/** @method post */
export function postSessionArchiveClear(options?: FetchOptions) {
  return ofetch<void>("/session/archive/clear", { baseURL, method: "post", ...options });
}

/** @method get */
export function getSessionArchive(options?: FetchOptions) {
  return ofetch<Types.ArchivedListPayload>("/session/archive", { baseURL, method: "get", ...options });
}

/** @method post */
export function postSessionArchive(body: Types.SessionIdBody, options?: FetchOptions) {
  return ofetch<void>("/session/archive", { baseURL, method: "post", body, ...options });
}

/** @method delete */
export function deleteSessionArchive(body: Types.SessionIdBody, options?: FetchOptions) {
  return ofetch<void>("/session/archive", { baseURL, method: "delete", body, ...options });
}

/** @method post */
export function postSessionArchiveRestore(body: Types.SessionIdBody, options?: FetchOptions) {
  return ofetch<void>("/session/archive/restore", { baseURL, method: "post", body, ...options });
}

/** @method post */
export function postSessionOpenPath(body: Types.SessionIdBody, options?: FetchOptions) {
  return ofetch<Types.OpenSessionDirResult>("/session/open/path", { baseURL, method: "post", body, ...options });
}

/** @method post */
export function postSessionWorkspaceArchive(body: Types.WorkspaceArchiveBody, options?: FetchOptions) {
  return ofetch<void>("/session/workspace/archive", { baseURL, method: "post", body, ...options });
}

/** @method delete */
export function deleteSessionWorkspaceArchive(body: Types.SessionIdsBody, options?: FetchOptions) {
  return ofetch<void>("/session/workspace/archive", { baseURL, method: "delete", body, ...options });
}
