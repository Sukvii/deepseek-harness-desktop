import type { ArchivedListPayload } from '../../../service/ledger.types'
import { defineEventHandler } from 'dsh-tauri'
import { ledger } from '../../../service/ledger'

export default defineEventHandler((): ArchivedListPayload => ledger.load())
