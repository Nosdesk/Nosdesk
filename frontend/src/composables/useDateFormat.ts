/**
 * Date Format Composable
 *
 * Provides reactive date formatting that automatically updates when:
 * - The date value changes
 * - User timezone preference changes
 * - Live updates for relative times
 */

import { computed, ref, onUnmounted, type Ref } from 'vue'
import {
  formatDate,
  formatRelativeTime,
  formatSmartDate,
  formatDateTime,
  formatTime
} from '@nosdesk/core/utils/dateUtils'
import { useDateStore } from '@nosdesk/core/stores/dateStore'

export interface UseDateFormatOptions {
  /** Update relative time every N seconds */
  liveUpdate?: number
  /** Format type */
  format?: 'short' | 'long' | 'relative' | 'smart' | 'dateTime' | 'time'
}

export function useDateFormat(
  dateSource: Ref<string | Date | null | undefined> | string | Date | null | undefined,
  options: UseDateFormatOptions = {}
) {
  const dateStore = useDateStore()

  const {
    liveUpdate = 0,
    format: formatType = 'short'
  } = options

  // Convert to ref if not already
  const dateRef = typeof dateSource === 'object' && dateSource !== null && 'value' in dateSource
    ? dateSource
    : ref(dateSource)

  // Tick ref for live updates
  const tick = ref(0)

  // Formatted date — recomputes when the date, the active
  // timezone, the active locale, or the tick changes. Reading
  // dateStore.locale here (even unused) is what makes Vue's
  // reactivity track it, so flipping locales in settings flows
  // through to every <DateCell> instance.
  const formatted = computed(() => {
    // Touch reactivity triggers.
    tick.value
     
    dateStore.locale

    const dateValue = dateRef.value

    switch (formatType) {
      case 'relative':
        return formatRelativeTime(dateValue)
      case 'smart':
        return formatSmartDate(dateValue)
      case 'dateTime':
        return formatDateTime(dateValue, dateStore.effectiveTimezone)
      case 'time':
        return formatTime(dateValue, dateStore.effectiveTimezone)
      case 'long':
        return formatDate(dateValue, 'MMMM d, yyyy h:mm a', dateStore.effectiveTimezone)
      case 'short':
      default:
        return formatDate(dateValue, undefined, dateStore.effectiveTimezone)
    }
  })

  // Live update for relative times
  let intervalId: ReturnType<typeof setInterval> | undefined

  if (liveUpdate > 0 && (formatType === 'relative' || formatType === 'smart')) {
    intervalId = setInterval(() => {
      tick.value++
    }, liveUpdate * 1000)
  }

  onUnmounted(() => {
    if (intervalId) {
      clearInterval(intervalId)
    }
  })

  return {
    formatted,
    timezone: computed(() => dateStore.effectiveTimezone)
  }
}
