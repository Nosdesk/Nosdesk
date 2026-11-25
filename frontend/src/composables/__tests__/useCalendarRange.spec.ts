import { beforeEach, describe, expect, it } from 'vitest'
import { computed, nextTick, ref } from 'vue'
import { useCalendarRange } from '@/composables/useCalendarRange'
import type { ResolvedView } from '@/composables/useTicketsViewResolution'

function view(id: string): ResolvedView {
  return { id } as ResolvedView
}

describe('useCalendarRange', () => {
  beforeEach(() => localStorage.clear())

  it('opens on a month, and keeps each view to the range chosen for it', async () => {
    const active = ref(view('calendar'))
    const range = useCalendarRange(computed(() => active.value))
    expect(range.value).toBe('month')

    range.value = 'agenda'
    await nextTick()
    active.value = view('saved-1')
    await nextTick()
    expect(range.value).toBe('month')

    active.value = view('calendar')
    await nextTick()
    expect(range.value).toBe('agenda')
    // A second visit reads it back.
    expect(useCalendarRange(computed(() => view('calendar'))).value).toBe('agenda')
  })
})
