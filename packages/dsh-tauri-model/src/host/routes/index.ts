import { defineRoutes } from 'dsh-tauri'
import openConfig from './config/open/post'
import endpointModels from './endpoint/models/get'
import presets from './presets/get'

export const routes = defineRoutes((disposer) => {
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-model/config/open' }, openConfig)
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-model/endpoint/models' }, endpointModels)
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-model/presets' }, presets)
})
