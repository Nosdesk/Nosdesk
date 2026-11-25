import type { Day } from '@nosdesk/core/utils/ticketDates'

/** A dated fact that is not a ticket (a warranty running out, an SLA
 *  target), fetched by the tickets view and passed in by day. */
export interface CalendarOverlay {
  /** Stable id for keying. */
  id: string
  /** `YYYY-MM-DD`; lookup is exact match. */
  date: Day
  kind: 'warranty_expiry' | 'maintenance' | 'os_cutoff' | 'sla_breach'
  label: string
  /** Where it opens, when it opens anywhere. */
  href?: string
}
