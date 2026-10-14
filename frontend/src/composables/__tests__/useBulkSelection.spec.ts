import { describe, expect, it } from 'vitest'
import { ref } from 'vue'
import { useBulkSelection } from '@/composables/useBulkSelection'

// E (id 10) is older than F (id 11).
const E = { id: 10 }
const F = { id: 11 }

function selection(order: { id: number }[]) {
  return useBulkSelection({ items: ref(order), cacheKey: ref('k') })
}

describe('useBulkSelection lastTickedId', () => {
  it('is empty after select-all, whatever the list order', () => {
    for (const order of [
      [F, E],
      [E, F],
    ]) {
      const s = selection(order)
      s.toggleAllOnPage()
      expect(s.selectedCount.value).toBe(2)
      expect(s.lastTickedId.value).toBeNull()
    }
  })

  it('is the ticket ticked on last', () => {
    const s = selection([F, E])
    s.toggle('10')
    s.toggle('11')
    expect(s.lastTickedId.value).toBe('11')
  })

  it('is empty after a shift-range, which adds in list order', () => {
    const s = selection([F, E])
    s.toggle('11')
    s.toggle('10', { shiftKey: true })
    expect(s.selectedCount.value).toBe(2)
    expect(s.lastTickedId.value).toBeNull()
  })

  it('forgets a ticket ticked off again', () => {
    const s = selection([F, E])
    s.toggle('10')
    s.toggle('11')
    s.toggle('11')
    expect(s.lastTickedId.value).toBeNull()
  })
})
