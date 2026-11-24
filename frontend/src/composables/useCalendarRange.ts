/**
 * Month or Agenda for a calendar view. Each view lands on the range
 * chosen for it on this device (kept in localStorage the way its sort
 * is), else a month.
 */
import { ref, watch, type ComputedRef, type Ref } from 'vue'
import type { ResolvedView } from '@/composables/useTicketsViewResolution'
import type { CalendarRange } from '@nosdesk/core/sync/views/calendar'

const STORAGE_PREFIX = 'tickets-calendar-range:'

function loadRange(viewId: string): CalendarRange {
  try {
    const raw = localStorage.getItem(`${STORAGE_PREFIX}${viewId}`)
    return raw === 'agenda' ? 'agenda' : 'month'
  } catch {
    return 'month'
  }
}

function persistRange(viewId: string, range: CalendarRange): void {
  try {
    localStorage.setItem(`${STORAGE_PREFIX}${viewId}`, range)
  } catch {
    // Storage blocked: the choice lasts for this visit.
  }
}

export function useCalendarRange(activeView: ComputedRef<ResolvedView>): Ref<CalendarRange> {
  const range = ref<CalendarRange>(loadRange(activeView.value.id))
  let viewId = activeView.value.id

  watch(activeView, (next) => {
    if (next.id === viewId) return
    viewId = next.id
    range.value = loadRange(next.id)
  })
  watch(range, (next) => {
    if (next !== loadRange(viewId)) persistRange(viewId, next)
  })

  return range
}
