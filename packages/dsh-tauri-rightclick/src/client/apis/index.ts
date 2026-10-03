/*
 * @title dsh-tauri-rightclick
 * @swagger 2.0
 * @version 0.0.0
 */

import type { FetchOptions } from "dsh-tauri/client";
import { ofetch } from "dsh-tauri/client";
import type * as Types from "./index.type";

export const baseURL = "/api/desktop/dsh-tauri-rightclick";

/** @method post */
export function postOpenPath(body: Types.PostOpenPathBody, options?: FetchOptions) {
  return ofetch<Types.OperationResult>("/open/path", { baseURL, method: "post", body, ...options });
}

/** @method post */
export function postOpenUrl(body: Types.PostOpenUrlBody, options?: FetchOptions) {
  return ofetch<Types.OperationResult>("/open/url", { baseURL, method: "post", body, ...options });
}
