import { defineRoutes } from 'dsh-tauri'
import historyDelete from './history/delete'
import history from './history/get'
import options from './options/get'
import recover from './runs/recover/post'
import tasksDelete from './tasks/delete'
import tasks from './tasks/get'
import tasksCreate from './tasks/post'
import tasksUpdate from './tasks/put'
import tasksRun from './tasks/run/post'
import tasksToggle from './tasks/toggle/post'

export const routes = defineRoutes((disposer) => {
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/tasks' }, tasks)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/tasks' }, tasksCreate)
  disposer.put({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/tasks' }, tasksUpdate)
  disposer.delete({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/tasks' }, tasksDelete)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/tasks/toggle' }, tasksToggle)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/tasks/run' }, tasksRun)
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/history' }, history)
  disposer.delete({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/history' }, historyDelete)
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/options' }, options)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-scheduler/runs/recover' }, recover)
})
