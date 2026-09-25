import type { StatusPillTone } from '@/components/common/statusPillTone'

import type { StateCategory } from './service'

/** Pill tone for a workflow-state category, as the requester sees it. */
export function stateTone(category: StateCategory): StatusPillTone {
  switch (category) {
    case 'active':
      return 'accent'
    case 'in_review':
      return 'caution'
    case 'done':
      return 'positive'
    case 'triage':
    case 'backlog':
      return 'info'
    default:
      return 'neutral'
  }
}
