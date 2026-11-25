/**
 * Today's date in the planning timezone, as a reactive `YYYY-MM-DD`.
 *
 * Due chips, due buckets and the overdue colour all read it, so a
 * list left open overnight moves its "Today" jobs to Overdue on its
 * own. One shared minute tick (plus a check when the tab comes back
 * into view) serves every consumer; the computed only changes when
 * the day does, so nothing re-renders on the other ticks.
 */
import { computed, getCurrentScope, onScopeDispose, ref, type ComputedRef } from 'vue'
import { planningTimezone, todayIn, type Day } from '@nosdesk/core/utils/ticketDates'

const TICK_INTERVAL_MS = 60_000
const tickNow = ref(Date.now())
let tickHandle: ReturnType<typeof setInterval> | null = null
let subscribers = 0

function onVisible(): void {
  if (document.visibilityState === 'visible') tickNow.value = Date.now()
}

function subscribe(): void {
  subscribers += 1
  if (tickHandle !== null) return
  tickNow.value = Date.now()
  tickHandle = setInterval(() => {
    tickNow.value = Date.now()
  }, TICK_INTERVAL_MS)
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisible)
}

function unsubscribe(): void {
  subscribers = Math.max(0, subscribers - 1)
  if (subscribers > 0 || tickHandle === null) return
  clearInterval(tickHandle)
  tickHandle = null
  if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisible)
}

export function useToday(): ComputedRef<Day> {
  if (getCurrentScope()) {
    subscribe()
    onScopeDispose(unsubscribe)
  }
  return computed(() => todayIn(planningTimezone(), new Date(tickNow.value)))
}
