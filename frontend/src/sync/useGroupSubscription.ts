import { onScopeDispose, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { logger } from '@nosdesk/core/utils/logger'
import { subscribe, unsubscribe } from './lifecycle'

/**
 * How long a group stays subscribed after the last view holding it leaves.
 * Going back and forth between a list and a ticket costs nothing, and the
 * group list each delta poll sends stays bounded.
 */
export const GROUP_RELEASE_GRACE_MS = 5 * 60_000

/** Views holding each group, across every `useGroupSubscription`. */
const holders = new Map<string, number>()
/** Unsubscribes waiting out their grace, by group. */
const releases = new Map<string, ReturnType<typeof setTimeout>>()
/** Bumped by `cancelGroupReleases`; a hold taken before it releases nothing. */
let session = 0

function hold(group: string): Promise<void> {
  holders.set(group, (holders.get(group) ?? 0) + 1)
  const pending = releases.get(group)
  if (pending !== undefined) {
    clearTimeout(pending)
    releases.delete(group)
  }
  return subscribe(group)
}

function release(group: string, heldIn: number): void {
  if (heldIn !== session) return
  const left = (holders.get(group) ?? 1) - 1
  if (left > 0) {
    holders.set(group, left)
    return
  }
  holders.delete(group)
  releases.set(
    group,
    setTimeout(() => {
      releases.delete(group)
      void unsubscribe(group)
    }, GROUP_RELEASE_GRACE_MS),
  )
}

/**
 * Drop every hold and pending unsubscribe. Called by the sync runtime's
 * tear-down: the pool's subscriptions are gone with it, and a timer left
 * running would unsubscribe the same group name in the next workspace.
 */
export function cancelGroupReleases(): void {
  for (const timer of releases.values()) clearTimeout(timer)
  releases.clear()
  holders.clear()
  session++
}

/**
 * Keep the sync pool subscribed to `group` (`ticket:N`, `project:P`) while the
 * calling component lives, following the getter when it changes; null holds
 * nothing. Leaving unsubscribes after a grace period, canceled when any view
 * holds the group again within it. Not for the workspace group, which
 * `useWorkspaceGroupSubscription` holds for the session.
 *
 * `settled()` resolves once the subscribe for the current group has, so a
 * view can await the group's rows before deciding something is missing.
 */
export function useGroupSubscription(group: MaybeRefOrGetter<string | null | undefined>): {
  settled: () => Promise<void>
} {
  let held: string | null = null
  let heldIn = session
  let subscribed: Promise<void> = Promise.resolve()

  function follow(): Promise<void> {
    const next = toValue(group) || null
    if (next === held && heldIn === session) return subscribed
    if (held !== null) release(held, heldIn)
    held = next
    heldIn = session
    subscribed = next !== null ? hold(next) : Promise.resolve()
    return subscribed
  }

  watch(
    () => toValue(group),
    () => {
      follow().catch((error: unknown) => {
        logger.warn('sync: subscribing a view group failed', { group: held, error })
      })
    },
    { immediate: true },
  )
  onScopeDispose(() => {
    if (held !== null) release(held, heldIn)
    held = null
  })

  return { settled: follow }
}
