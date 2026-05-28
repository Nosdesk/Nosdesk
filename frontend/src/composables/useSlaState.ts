/**
 * Compute the visual SLA state for a card. Centralises the tone
 * class / label / time-bar fraction logic that previously lived
 * inline in TicketsTable (compact pill text + tone) and
 * TicketPreviewPane (full bar with detail line).
 *
 * Returns null when the card has no SLA — consumers conditionally
 * render the SLA chrome only when this resolves.
 *
 * **Live derivation.** The countdown text, the at-risk transition,
 * and the breach pre-flip are all derived from `start_at` +
 * `target_at` + the current wall clock — not from the server's
 * pre-baked `seconds_remaining` / `pill_color` / `breached` flag.
 * That keeps the pill current between server-emitted updates: the
 * "3h 12m" counts down, the green→amber→red transitions fire when
 * they actually happen, and a freshly-breached ticket flips red
 * immediately (the server's breach-detection job catches up within
 * 60s and emits an authoritative `ticket.sla_updated` that is
 * consistent with what we already showed). `useSlaState` shares one
 * 30s tick ref across every active pill via a ref-counted
 * subscription so we run one timer regardless of how many tickets
 * are on screen.
 */
import { computed, onMounted, onUnmounted, ref, type ComputedRef, type Ref } from 'vue'
import type { CardData } from '@/sync/views/types'

// Single shared tick: re-emits the wall clock so every consumer's
// computed re-evaluates together. 30s is a sweet spot — fast enough
// that the "3h" → "2h 59m" transition isn't visibly stale, slow
// enough that we're not waking the main thread every second on every
// open ticket view.
const TICK_INTERVAL_MS = 30_000
const tickNow = ref(Date.now())
let tickHandle: ReturnType<typeof setInterval> | null = null
let tickSubs = 0

function subscribeTick(): void {
  tickSubs += 1
  if (tickHandle === null) {
    tickHandle = setInterval(() => {
      tickNow.value = Date.now()
    }, TICK_INTERVAL_MS)
  }
}

function unsubscribeTick(): void {
  tickSubs = Math.max(0, tickSubs - 1)
  if (tickSubs === 0 && tickHandle !== null) {
    clearInterval(tickHandle)
    tickHandle = null
  }
}

export interface SlaState {
  /** Compact pill text — used in the table column. */
  compactLabel: string
  /** "Breached" / "At risk" / "On track" / "Paused" — used as the
   * leading word in the preview pane's SLA section. */
  statusLabel: string
  /** Tone class for any text rendered in this state. */
  toneClass: string
  /** Tone class for the bar fill in the preview pane. */
  barClass: string
  /** 0..1 fill fraction for the urgency bar. */
  fraction: number
  /** Detail line for the preview pane, eg.
   * "3 hours remaining · target Tue 12:00 PM". */
  detail: string
  /** Formatted target timestamp (full, eg. "Tue, May 7 12:00 PM"). */
  target: string
  /** True when this state should treat the SLA as broken. */
  breached: boolean
  /** True when the timer is paused. */
  paused: boolean
}

function fullDateTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  })
}

function compactRemaining(seconds: number): string {
  if (seconds < 3600) return `${Math.ceil(seconds / 60)}m`
  if (seconds < 86_400) return `${Math.ceil(seconds / 3600)}h`
  return `${Math.ceil(seconds / 86_400)}d`
}

function detailRemaining(seconds: number): string {
  if (seconds < 3600) return `${Math.ceil(seconds / 60)} min remaining`
  if (seconds < 86_400) return `${Math.ceil(seconds / 3600)} hours remaining`
  return `${Math.ceil(seconds / 86_400)} days remaining`
}

/** Seconds remaining until the target, computed against `now`.
 *  Negative when the target has passed. Source of truth for the
 *  pill's countdown text + the client-side breach pre-flip. */
function liveSecondsRemaining(targetIso: string, now: number): number {
  return Math.floor((new Date(targetIso).getTime() - now) / 1000)
}

/** True progress fraction (0..1) of elapsed time over the timer's
 *  window. Replaces the bucket-based urgency guess we used before
 *  start_at landed in the payload — the bar now literally tracks how
 *  far through the window the ticket is. Clamped to 0..1 so a stale
 *  clock or out-of-order computation can't produce a runaway value. */
function progressFraction(startIso: string, targetIso: string, now: number): number {
  const start = new Date(startIso).getTime()
  const target = new Date(targetIso).getTime()
  const windowMs = target - start
  if (windowMs <= 0) return 1
  const elapsed = now - start
  return Math.min(1, Math.max(0, elapsed / windowMs))
}

/**
 * Standalone SLA shape, decoupled from CardData. Used by the
 * ticket detail view (whose payload carries the same SLA pill
 * the bootstrap streamer emits, but without the full CardData
 * envelope around it).
 */
export type SlaPayload = NonNullable<CardData['sla']>

export function deriveSlaState(
  card: CardData | null,
  now?: number,
): SlaState | null
export function deriveSlaState(
  sla: SlaPayload | null | undefined,
  now?: number,
): SlaState | null
export function deriveSlaState(
  source: CardData | SlaPayload | null | undefined,
  now: number = Date.now(),
): SlaState | null {
  if (!source) return null
  // Discriminate on shape: CardData carries an `sla` field;
  // SlaPayload has `target_at` directly. Treat both as a single
  // entry point so callers don't need to know which one they
  // hold.
  const sla =
    'target_at' in source
      ? (source as SlaPayload)
      : ((source as CardData).sla as SlaPayload | null | undefined)
  if (!sla) return null
  const target = fullDateTime(sla.target_at)

  // Met-on-time short-circuits: the timer is done, render as done.
  // `met_at` is server-authoritative (set when first_response_at
  // lands); we don't predict response client-side.
  if (sla.met_at) {
    return {
      compactLabel: 'Met',
      statusLabel: 'Met',
      toneClass: 'text-emerald-600 dark:text-emerald-400',
      barClass: 'bg-emerald-500',
      fraction: 1,
      detail: `Met ${fullDateTime(sla.met_at)}`,
      target,
      breached: false,
      paused: false,
    }
  }

  // Paused freezes the visual — the server's `paused` flag is the
  // source of truth (we don't have the workflow_state category here
  // to derive pause client-side).
  if (sla.paused) {
    return {
      compactLabel: 'Paused',
      statusLabel: 'Paused',
      toneClass: 'text-zinc-500 dark:text-zinc-400',
      barClass: 'bg-zinc-400',
      fraction: 0.5,
      detail: `Target ${target}`,
      target,
      breached: false,
      paused: true,
    }
  }

  const remaining = liveSecondsRemaining(sla.target_at, now)

  // Effective-breached: trust the server's stamp when present, but
  // also pre-flip when the live clock has crossed `target_at`. The
  // breach-detection job catches up within 60s and emits an
  // authoritative sla_updated that is consistent with what we already
  // showed; pre-flipping avoids a 60s window of stale on-track tone.
  const effectiveBreached = sla.breached || remaining < 0
  if (effectiveBreached) {
    return {
      compactLabel: 'Breached',
      statusLabel: 'Breached',
      toneClass: 'text-rose-600 dark:text-rose-400',
      barClass: 'bg-rose-500',
      fraction: 1,
      detail: `Past target · ${target}`,
      target,
      breached: true,
      paused: false,
    }
  }

  const compact = compactRemaining(remaining)
  const detail = `${detailRemaining(remaining)} · target ${target}`
  const fraction = progressFraction(sla.start_at, sla.target_at, now)

  // At-risk client-side: within 25% of the window remaining flips
  // amber. Computed from start_at + target_at + now so the green →
  // amber transition is live, not bounded by the next server emit.
  const windowSeconds =
    (new Date(sla.target_at).getTime() - new Date(sla.start_at).getTime()) / 1000
  const atRisk = remaining * 4 < windowSeconds

  if (atRisk) {
    return {
      compactLabel: compact,
      statusLabel: 'At risk',
      toneClass: 'text-amber-600 dark:text-amber-400',
      barClass: 'bg-amber-500',
      fraction,
      detail,
      target,
      breached: false,
      paused: false,
    }
  }

  return {
    compactLabel: compact,
    statusLabel: 'On track',
    toneClass: 'text-emerald-600 dark:text-emerald-400',
    barClass: 'bg-emerald-500',
    fraction,
    detail,
    target,
    breached: false,
    paused: false,
  }
}

/** Reactive form: pass a ref / getter, get a computed SlaState
 *  that re-evaluates when the card changes AND every 30s as the
 *  shared tick fires — so the countdown text, the at-risk transition
 *  and the breach pre-flip all stay current without waiting on a
 *  server-emitted update. */
export function useSlaState(
  card: Ref<CardData | null> | (() => CardData | null),
): ComputedRef<SlaState | null> {
  onMounted(subscribeTick)
  onUnmounted(unsubscribeTick)
  return computed(() => {
    const c = typeof card === 'function' ? card() : card.value
    return deriveSlaState(c, tickNow.value)
  })
}

/**
 * Derive both the response and resolution timers' render states. Used
 * by the preview pane to stack them. Each sub-timer is run through the
 * same `deriveSlaState` derivation, so the rendered tone / label
 * vocabulary is consistent with the compact pill. Either field is
 * `null` when its target isn't configured on the matched policy (or
 * when the card has no SLA at all). `now` is threaded through so a
 * reactive caller (the preview pane wires this to the shared tick)
 * gets the same live behaviour as the compact pill.
 */
export function deriveSlaTimers(
  source: CardData | SlaPayload | null | undefined,
  now: number = Date.now(),
): { response: SlaState | null; resolution: SlaState | null } {
  if (!source) return { response: null, resolution: null }
  const sla =
    'target_at' in source
      ? (source as SlaPayload)
      : ((source as CardData).sla as SlaPayload | null | undefined)
  if (!sla) return { response: null, resolution: null }
  return {
    response: sla.response ? deriveSlaState(sla.response, now) : null,
    resolution: sla.resolution ? deriveSlaState(sla.resolution, now) : null,
  }
}

/** Reactive form of [`deriveSlaTimers`] — pairs the same shared 30s
 *  tick that drives `useSlaState` so the stacked preview-pane rows
 *  stay perfectly in sync with the compact list pill. */
export function useSlaTimers(
  card: Ref<CardData | null> | (() => CardData | null),
): ComputedRef<{ response: SlaState | null; resolution: SlaState | null }> {
  onMounted(subscribeTick)
  onUnmounted(unsubscribeTick)
  return computed(() => {
    const c = typeof card === 'function' ? card() : card.value
    return deriveSlaTimers(c, tickNow.value)
  })
}
