/*
 * @title dsh-tauri-worktree
 * @swagger 2.0
 * @version 0.0.0
 */

import type { FetchOptions } from "dsh-tauri/client";
import { ofetch } from "dsh-tauri/client";
import type * as Types from "./index.type";

export const baseURL = "/api/desktop/dsh-tauri-worktree";

/** @method get */
export function getBindings(options?: FetchOptions) {
  return ofetch<Types.WorktreeBindings>("/bindings", { baseURL, method: "get", ...options });
}

/** @method post */
export function postBindings(body: Types.AttachBody, options?: FetchOptions) {
  return ofetch<Types.WorktreeAttach>("/bindings", { baseURL, method: "post", body, ...options });
}

/** @method post */
export function postCheckouts(body: Types.CheckoutBody, options?: FetchOptions) {
  return ofetch<Types.WorktreeCheckout>("/checkouts", { baseURL, method: "post", body, ...options });
}

/** @method post */
export function postWorktree(body: Types.CreateBody, options?: FetchOptions) {
  return ofetch<Types.WorktreeCreate>("", { baseURL, method: "post", body, ...options });
}

/** @method delete */
export function deleteWorktree(body: Types.DiscardBody, options?: FetchOptions) {
  return ofetch<Types.WorktreeDiscard>("", { baseURL, method: "delete", body, ...options });
}

/** @method get */
export function getStatus(params?: Types.GetStatusQuery, options?: FetchOptions) {
  return ofetch<Types.WorktreeStatus>("/status", { baseURL, method: "get", params, ...options });
}
