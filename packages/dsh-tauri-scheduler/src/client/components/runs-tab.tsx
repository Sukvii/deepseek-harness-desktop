import type { ReactElement } from 'react'
import type { LocaleKey, Translate } from '../locales/index.types'
import type { RunView } from '../types'
import { Action, Alarm, Card, CircleCheck, CircleDashed, CircleStop, CircleXmark, Dot, Icon, Text, TrashBin } from 'dsh-tauri-ui/client'
import { formatLocalTime, isRunUnread } from './schedule.utils'

export interface RunsTabProps {
  t: Translate
  runs: readonly RunView[]
  readAt: number
  readIds: readonly string[]
  emptyLabel: string
  onOpen: (run: RunView) => void
  onDelete: (id: string) => void
}

const STATUS_KEYS: Record<RunView['status'], LocaleKey> = {
  succeeded: 'succeeded',
  failed: 'failed',
  interrupted: 'interrupted',
  skipped: 'skipped',
  cancelled: 'cancelled',
  queued: 'queued',
  running: 'running',
}

const STATUS_ICONS = {
  succeeded: CircleCheck,
  failed: CircleXmark,
  interrupted: CircleXmark,
  cancelled: CircleStop,
  skipped: CircleDashed,
  queued: CircleDashed,
  running: Alarm,
}

export function RunsTab({ t, runs, readAt, readIds, emptyLabel, onOpen, onDelete }: RunsTabProps): ReactElement {
  if (runs.length === 0)
    return <Text size="sm" tone="tertiary" className="py-[48px] text-center">{emptyLabel}</Text>
  return (
    <Card.List className="gap-[8px]">
      {runs.map(run => (
        <Card variant="link" key={run.id} className="box-border mx-0 flex justify-between items-center gap-[10px] w-full min-w-0 h-[60px] px-[12px] py-[10px] rounded-[10px] text-inherit [font-family:inherit] text-[13px] leading-[20px] text-left cursor-pointer overflow-hidden" onClick={() => onOpen(run)}>
          <div style={{ height: 36 }}>
            <span
              className="flex-none inline-flex items-center justify-center mt-[2px] w-[16px] h-[16px] text-[16px] text-business data-[status=succeeded]:text-success data-[status=failed]:text-error data-[status=interrupted]:text-error data-[status=running]:text-secondary data-[status=queued]:text-secondary data-[status=cancelled]:text-tertiary data-[status=skipped]:text-tertiary"
              data-status={run.status}
              role="img"
              aria-label={t(STATUS_KEYS[run.status])}
              title={t(STATUS_KEYS[run.status])}
            >
              <Icon as={STATUS_ICONS[run.status]} />
            </span>
          </div>
          <div style={{ flex: 1, minWidth: 0 }}>
            <Card.Title className="flex items-center gap-[8px] text-[13px] leading-[18px]" title={run.taskName}>
              {run.taskName}
              {isRunUnread(run, readAt, readIds) ? <Dot state="done" /> : null}
            </Card.Title>
            <div className="flex items-center gap-[10px] min-w-0">
              <Card.Description className="flex-1 min-w-0 text-[12px] line-clamp-none truncate">{formatLocalTime(run.startedAt) ?? ''}</Card.Description>
            </div>
          </div>
          <Action
            variant="action"
            icon={<Icon as={TrashBin} size={12} />}
            aria-label={t('deleteRun')}
            onClick={(event) => {
              event.stopPropagation()
              onDelete(run.id)
            }}
          />
        </Card>
      ))}
    </Card.List>
  )
}
