import type { ClientContext, PanelHandle } from 'dsh-tauri/client'
import type { Translate } from '../locales/index.types'
import { Panel } from 'dsh-tauri-ui/client'
import { definePanel, defineRegister } from 'dsh-tauri/client'
import { SchedulerNavIcon } from '../components/scheduler-nav-icon'
import { SchedulerPanel } from '../components/scheduler-panel'
import { PANEL_ACTION_ORDER, PANEL_ID, REFRESH_INTERVAL_MS } from '../constants'
import { locale } from '../locales'
import { loadScheduler } from '../service/scheduler'
import { store } from '../store'

export const panelFeature = defineRegister<ClientContext>((controller, ctx, adapter) => {
  const t: Translate = locale.text
  const holder: { current?: PanelHandle } = {}

  // 侧边栏未读角标在面板关闭时也要跟上新运行，所以拉取轮询放在注册层；
  // 面板自己只负责首屏（含对话框选项）与回焦刷新。
  void loadScheduler(false)
  const timer = setInterval(() => {
    void loadScheduler(false)
  }, REFRESH_INTERVAL_MS)
  controller.add(() => clearInterval(timer))

  // 在会话区点开某条运行记录的会话时，同步消掉这条记录的未读。
  const sessionList = adapter.sessionList()
  if (sessionList !== undefined) {
    controller.add(sessionList.subscribe(() => {
      const current = adapter.sessionList()?.current
      if (current !== undefined)
        store.scheduler.markSessionRead(current)
    }))
  }

  holder.current = definePanel(ctx, {
    id: PANEL_ID,
    order: PANEL_ACTION_ORDER,
    locale: locale.NS,
    label: () => locale.text('scheduler'),
    icon: props => <SchedulerNavIcon size={props.size} />,
    render: () => (
      <Panel>
        <SchedulerPanel
          t={t}
          onViaChat={() => {
            store.prefill.set(locale.text('chatPrompt'))
            holder.current?.close()
          }}
          onOpenSession={(sessionId) => {
            // 归档的会话会从官方活动列表里消失，放行只会落到空白初始页，所以先拦下。
            const listed = adapter.sessionList()?.ids
            if (listed !== undefined && !listed.includes(sessionId))
              return 'archived'
            if (adapter.openSession(sessionId).status === 'unavailable')
              return 'unavailable'
            holder.current?.close()
            return 'opened'
          }}
        />
      </Panel>
    ),
  })
  controller.add(holder.current.dispose)
})
