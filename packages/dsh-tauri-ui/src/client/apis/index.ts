/*
 * @title dsh-tauri-ui
 * @swagger 2.0
 * @version 0.0.0
 */

import type { FetchOptions } from "dsh-tauri/client";
import { ofetch } from "dsh-tauri/client";
import type * as Types from "./index.type";

export const baseURL = "/api/desktop/dsh-tauri-ui";

/** @method post */
export function postSessionResume(body: Types.PostSessionResumeBody, options?: FetchOptions) {
  return ofetch<Types.SessionResumeResponse>("/session/resume", { baseURL, method: "post", body, ...options });
}

/** @method get */
export function getUngrouped(options?: FetchOptions) {
  return ofetch<Types.UngroupedResponse>("/ungrouped", { baseURL, method: "get", ...options });
}
