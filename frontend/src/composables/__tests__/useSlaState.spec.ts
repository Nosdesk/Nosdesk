import { describe, expect, it } from 'vitest'
import { deriveSlaState, type SlaPayload } from '@/composables/useSlaState'

const NOW = Date.parse('2026-05-04T11:40:00Z')

function timer(over: Partial<SlaPayload>): SlaPayload {
  return {
    start_at: '2026-05-04T10:00:00Z',
    target_at: '2026-05-04T11:00:00Z',
    breached: false,
    paused: false,
    pill_color: 'green',
    ...over,
  } as SlaPayload
}

describe('deriveSlaState', () => {
  it('shows a response met after its target as breached, not met', () => {
    const state = deriveSlaState(
      timer({ met_at: '2026-05-04T11:30:00Z', breached: true, pill_color: 'red' }),
      NOW,
    )
    expect(state?.breached).toBe(true)
    expect(state?.statusLabel).toBe('Breached')
  })

  it('shows a response met before its target as met', () => {
    const state = deriveSlaState(timer({ met_at: '2026-05-04T10:30:00Z' }), NOW)
    expect(state?.breached).toBe(false)
    expect(state?.statusLabel).toBe('Met')
  })

  it('shows a breached timer as breached while the state pauses the clock', () => {
    const state = deriveSlaState(
      timer({ met_at: '2026-05-04T11:30:00Z', breached: true, paused: true, pill_color: 'red' }),
      NOW,
    )
    expect(state?.breached).toBe(true)
  })

  it('flips to breached once the live clock passes the target', () => {
    const state = deriveSlaState(timer({ target_at: '2026-05-04T11:39:00Z' }), NOW)
    expect(state?.breached).toBe(true)
  })

  it('stays red but counts down to the next target after another timer breached', () => {
    // The response was met late; the flattened timer is the resolution,
    // due at 14:00, and the server marks the ticket breached.
    const state = deriveSlaState(
      timer({ target_at: '2026-05-04T14:00:00Z', breached: true, pill_color: 'red' }),
      NOW,
    )
    expect(state?.breached).toBe(true)
    expect(state?.statusLabel).toBe('Breached')
    expect(state?.toneClass).toContain('rose')
    expect(state?.compactLabel).toBe('3h')
  })
})
