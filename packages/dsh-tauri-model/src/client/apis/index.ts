/*
 * @title dsh-tauri-model
 * @swagger 2.0
 * @version 0.0.0
 */

import type { FetchOptions } from "dsh-tauri/client";
import { ofetch } from "dsh-tauri/client";
import type * as Types from "./index.type";

export const baseURL = "/api/desktop/dsh-tauri-model";

/** @method post */
export function postConfigOpen(options?: FetchOptions) {
  return ofetch<Types.OpenModelsConfigResponse>("/config/open", { baseURL, method: "post", ...options });
}

/** @method get */
export function getEndpointModels(params?: Types.GetEndpointModelsQuery, options?: FetchOptions) {
  return ofetch<Types.EndpointModelsResponse>("/endpoint/models", { baseURL, method: "get", params, ...options });
}

/** @method get */
export function getPresets(params?: Types.GetPresetsQuery, options?: FetchOptions) {
  return ofetch<Types.PresetsResponse>("/presets", { baseURL, method: "get", params, ...options });
}
