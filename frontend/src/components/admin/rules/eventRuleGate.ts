/**
 * Whether the server runs rules with an event trigger (ticket created,
 * updated or replied). Without it, an event rule can be built and saved as
 * a draft but not put in a dry run or live, since it would never run.
 * Manual rules aren't affected.
 */
import type { RuleState, RuleTriggerKind } from '@nosdesk/core/types/rule'

export const EVENT_RULES_RUN = true

/** The state changes a rule's state controls offer. */
export interface PromoteControls {
  /** Start a dry run: the rule runs in shadow and logs what it would do. */
  dryRun: boolean
  goLive: boolean
  /** Going live skips the dry run, so the admin confirms they checked the
   *  rule by hand first. */
  goLiveNeedsCheck: boolean
  pause: boolean
  /** An event rule that can't go further until event triggers run. */
  waitingForEvents: boolean
}

export function promoteControls(
  trigger: RuleTriggerKind,
  state: RuleState,
  eventRulesRun: boolean,
): PromoteControls {
  const parked = state === 'draft' || state === 'paused'
  const running = state === 'live' || state === 'dry_run'
  if (trigger === 'manual') {
    return { dryRun: false, goLive: parked, goLiveNeedsCheck: false, pause: state === 'live', waitingForEvents: false }
  }
  if (!eventRulesRun) {
    return { dryRun: false, goLive: false, goLiveNeedsCheck: false, pause: running, waitingForEvents: true }
  }
  return {
    dryRun: parked || state === 'live',
    goLive: parked || state === 'dry_run',
    goLiveNeedsCheck: parked,
    pause: running,
    waitingForEvents: false,
  }
}
