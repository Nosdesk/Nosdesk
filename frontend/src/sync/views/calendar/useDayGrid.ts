// Keys for a grid of days, shared by the full month and the compact one.
// One day is in the tab order; arrows move it a day or a week at a time,
// Home and End run to the ends of the week, Page Up and Page Down change
// month, and the grid follows the day into a neighbouring month.
import { nextTick, type Ref } from 'vue'
import type { Day } from '@nosdesk/core/utils/ticketDates'
import { shiftDay, shiftDayByMonths } from '@nosdesk/core/sync/views/calendar'

export function useDayGrid(options: {
  root: Ref<HTMLElement | null>
  focusDay: () => Day
  weeks: () => Day[][]
  /** Move the roving day; the parent turns the month if it has to. */
  move: (day: Day) => void
  activate: (day: Day, cell: HTMLElement) => void
}) {
  function focusCell(day: Day): void {
    void nextTick(() => {
      options.root.value?.querySelector<HTMLElement>(`[data-day="${day}"]`)?.focus()
    })
  }

  function onKeydown(event: KeyboardEvent): void {
    const cell = event.currentTarget as HTMLElement
    if (event.target !== cell) return
    const day = options.focusDay()
    const week = options.weeks().find((w) => w.includes(day))
    let next: Day | null = null
    switch (event.key) {
      case 'ArrowLeft': next = shiftDay(day, -1); break
      case 'ArrowRight': next = shiftDay(day, 1); break
      case 'ArrowUp': next = shiftDay(day, -7); break
      case 'ArrowDown': next = shiftDay(day, 7); break
      case 'Home': next = week?.[0] ?? null; break
      case 'End': next = week?.[6] ?? null; break
      case 'PageUp': next = shiftDayByMonths(day, -1); break
      case 'PageDown': next = shiftDayByMonths(day, 1); break
      case 'Enter':
      case ' ':
        event.preventDefault()
        options.activate(day, cell)
        return
      default:
        return
    }
    if (!next) return
    event.preventDefault()
    options.move(next)
    focusCell(next)
  }

  return { onKeydown, focusCell }
}
