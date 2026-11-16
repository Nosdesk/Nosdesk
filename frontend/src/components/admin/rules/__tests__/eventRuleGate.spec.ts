import { describe, expect, it } from 'vitest'
import type { RuleState, RuleTriggerKind } from '@nosdesk/core/types/rule'
import { EVENT_RULES_RUN, promoteControls } from '../eventRuleGate'

const offered = (trigger: RuleTriggerKind, state: RuleState, run: boolean) => {
  const c = promoteControls(trigger, state, run)
  return [c.dryRun && 'dry_run', c.goLive && (c.goLiveNeedsCheck ? 'live_checked' : 'live'), c.pause && 'paused']
    .filter(Boolean)
    .join(',')
}

describe('promoteControls', () => {
  it('runs event rules', () => {
    expect(EVENT_RULES_RUN).toBe(true)
  })

  it('keeps event rules in draft while event rules do not run', () => {
    for (const trigger of ['ticket_created', 'ticket_updated', 'ticket_replied', 'time_elapsed'] as RuleTriggerKind[]) {
      expect(offered(trigger, 'draft', false)).toBe('')
      expect(offered(trigger, 'paused', false)).toBe('')
      // One already running (set through the API) can still be stopped.
      expect(offered(trigger, 'live', false)).toBe('paused')
      expect(offered(trigger, 'dry_run', false)).toBe('paused')
      expect(promoteControls(trigger, 'draft', false).waitingForEvents).toBe(true)
    }
  })

  it('leaves manual rules as they were, whatever the switch says', () => {
    for (const run of [false, true]) {
      expect(offered('manual', 'draft', run)).toBe('live')
      expect(offered('manual', 'paused', run)).toBe('live')
      expect(offered('manual', 'live', run)).toBe('paused')
      expect(promoteControls('manual', 'draft', run).waitingForEvents).toBe(false)
    }
  })

  it('once event rules run, offers a dry run first and going live straight away only with a check', () => {
    expect(offered('ticket_created', 'draft', true)).toBe('dry_run,live_checked')
    expect(offered('ticket_created', 'paused', true)).toBe('dry_run,live_checked')
    expect(offered('ticket_created', 'dry_run', true)).toBe('live,paused')
    expect(offered('ticket_created', 'live', true)).toBe('dry_run,paused')
  })
})
