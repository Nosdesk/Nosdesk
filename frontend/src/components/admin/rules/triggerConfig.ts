/**
 * A rule's `trigger_config` as the editor's form holds it, and back. An
 * updated trigger names the fields whose change runs it (none named: any
 * change); a replied trigger names whose reply runs it; a plugin event
 * trigger names the plugin and the event of its that runs it; a time-elapsed
 * trigger names a clock, how long after it started (or before the due date)
 * the rule runs, and how often it runs again, if at all. The form holds the
 * times as an amount and a unit; the config holds minutes.
 */
import type { RuleReplyKind, RuleTriggerConfig, RuleTriggerKind } from '@nosdesk/core/types/rule'

/** The clocks a time-elapsed rule can wait on, as the server names them
 *  (`services/rules/clocks.rs`). The SLA due time isn't one yet. */
export const ELAPSED_CLOCKS = [
  'since_created',
  'since_status_change',
  'since_assigned',
  'since_last_agent_reply',
  'since_last_customer_reply',
  'awaiting_requester',
  'until_due_at',
] as const
export type ElapsedClock = (typeof ELAPSED_CLOCKS)[number]

/** The longest time or repeat a rule takes: a year. */
export const MAX_ELAPSED_MINUTES = 365 * 24 * 60
/** The shortest repeat: an hour. */
export const MIN_REPEAT_MINUTES = 60

export type ElapsedUnit = 'minutes' | 'hours' | 'days'
export const UNIT_MINUTES: Record<ElapsedUnit, number> = { minutes: 1, hours: 60, days: 24 * 60 }
const UNITS_LARGEST_FIRST: ElapsedUnit[] = ['days', 'hours', 'minutes']

/** A time as the form shows it; `amount` is null while the field is empty. */
export interface ElapsedDuration {
  amount: number | null
  unit: ElapsedUnit
}

export interface TriggerSettings {
  changedFields: string[]
  replyKind: RuleReplyKind
  /** The plugin a plugin event rule runs on; empty until one is picked. */
  pluginUuid: string
  /** The plugin's event that runs it; empty until one is picked. */
  pluginEvent: string
  /** The clock a time-elapsed rule waits on; empty until a known one is
   *  picked. */
  clockName: string
  /** How long after the clock started the rule runs, or before the due
   *  date for `until_due_at`. */
  after: ElapsedDuration
  /** Whether it runs again while the clock keeps running. */
  repeat: boolean
  /** How often it runs again; kept while the repeat is off, so turning it
   *  back on shows it. */
  every: ElapsedDuration
}

const REPLY_KINDS: RuleReplyKind[] = ['customer', 'agent', 'any']

export const DEFAULT_REPLY_KIND: RuleReplyKind = 'customer'

/** The repeat a time rule offers when it's turned on. */
const DEFAULT_EVERY: ElapsedDuration = { amount: 1, unit: 'days' }

export const isElapsedClock = (name: unknown): name is ElapsedClock =>
  typeof name === 'string' && (ELAPSED_CLOCKS as readonly string[]).includes(name)

/** Minutes as the largest unit that divides them exactly. */
export function splitMinutes(minutes: number): ElapsedDuration {
  const unit = UNITS_LARGEST_FIRST.find((u) => minutes !== 0 && minutes % UNIT_MINUTES[u] === 0) ?? 'minutes'
  return { amount: minutes / UNIT_MINUTES[unit], unit }
}

export function toMinutes(duration: ElapsedDuration): number | null {
  return duration.amount == null ? null : duration.amount * UNIT_MINUTES[duration.unit]
}

export function readTriggerConfig(config: RuleTriggerConfig | null | undefined): TriggerSettings {
  const fields = config?.changed_fields_include
  const reply = config?.reply_kind
  const minutes = config?.minutes
  const every = config?.repeat_every_minutes
  return {
    changedFields: Array.isArray(fields) ? fields.filter((f): f is string => typeof f === 'string') : [],
    replyKind: reply && REPLY_KINDS.includes(reply) ? reply : DEFAULT_REPLY_KIND,
    pluginUuid: typeof config?.plugin_uuid === 'string' ? config.plugin_uuid : '',
    pluginEvent: typeof config?.event_type === 'string' ? config.event_type : '',
    clockName: isElapsedClock(config?.clock_name) ? config.clock_name : '',
    after: typeof minutes === 'number' ? splitMinutes(minutes) : { amount: null, unit: 'days' },
    repeat: typeof every === 'number',
    every: typeof every === 'number' ? splitMinutes(every) : { ...DEFAULT_EVERY },
  }
}

export function triggerConfigFor(kind: RuleTriggerKind, settings: TriggerSettings): RuleTriggerConfig {
  if (kind === 'ticket_updated' && settings.changedFields.length > 0) {
    return { changed_fields_include: [...settings.changedFields] }
  }
  if (kind === 'ticket_replied') return { reply_kind: settings.replyKind }
  if (kind === 'plugin_event') {
    return {
      ...(settings.pluginUuid ? { plugin_uuid: settings.pluginUuid } : {}),
      ...(settings.pluginEvent ? { event_type: settings.pluginEvent } : {}),
    }
  }
  if (kind === 'time_elapsed') {
    const minutes = toMinutes(settings.after)
    return {
      ...(settings.clockName ? { clock_name: settings.clockName } : {}),
      ...(minutes != null ? { minutes } : {}),
      repeat_every_minutes: settings.repeat ? toMinutes(settings.every) : null,
    }
  }
  return {}
}

/** What's wrong with each part of a time-elapsed trigger, as message keys.
 *  The ranges are the server's (`clocks::parse_trigger`). */
export interface ElapsedProblems {
  clock: string | null
  after: string | null
  repeat: string | null
}

const inRange = (minutes: number, least: number) =>
  Number.isInteger(minutes) && minutes >= least && minutes <= MAX_ELAPSED_MINUTES

export function elapsedProblems(settings: TriggerSettings): ElapsedProblems {
  const countsDown = settings.clockName === 'until_due_at'
  const after = toMinutes(settings.after)
  const every = toMinutes(settings.every)
  let afterProblem: string | null = null
  if (after == null) afterProblem = 'admin-rule-editor-elapsed-after-missing'
  else if (!inRange(after, countsDown ? 0 : 1)) {
    afterProblem = countsDown ? 'admin-rule-editor-elapsed-before-range' : 'admin-rule-editor-elapsed-after-range'
  }
  let repeatProblem: string | null = null
  if (settings.repeat && every == null) repeatProblem = 'admin-rule-editor-elapsed-repeat-missing'
  else if (settings.repeat && every != null && !inRange(every, MIN_REPEAT_MINUTES)) {
    repeatProblem = 'admin-rule-editor-elapsed-repeat-range'
  }
  return {
    clock: isElapsedClock(settings.clockName) ? null : 'admin-rule-editor-elapsed-clock-missing',
    after: afterProblem,
    repeat: repeatProblem,
  }
}

/** What the trigger still needs before the rule can be saved, as a message
 *  key: a plugin event rule names a plugin and one of its events; a
 *  time-elapsed rule names a clock and times the server takes. */
export function triggerProblem(kind: RuleTriggerKind, settings: TriggerSettings): string | null {
  if (kind === 'plugin_event' && (!settings.pluginUuid || !settings.pluginEvent)) {
    return 'admin-rule-editor-plugin-incomplete'
  }
  if (kind === 'time_elapsed') {
    const problems = elapsedProblems(settings)
    return problems.clock ?? problems.after ?? problems.repeat
  }
  return null
}

type Translate = (key: string, args?: Record<string, string | number>) => string

/** A number of minutes in words, in the largest unit that divides it. */
export function durationText(t: Translate, minutes: number): string {
  const { amount, unit } = splitMinutes(minutes)
  return t(`admin-rule-elapsed-duration-${unit}`, { count: amount ?? 0 })
}

/** When a time-elapsed rule runs, as a sentence ("When a ticket has waited
 *  3 days on the requester."), or null while its settings are unfinished. */
export function elapsedSummary(t: Translate, settings: TriggerSettings): string | null {
  if (triggerProblem('time_elapsed', settings)) return null
  const minutes = toMinutes(settings.after) ?? 0
  const when =
    settings.clockName === 'until_due_at' && minutes === 0
      ? t('admin-rule-elapsed-summary-until-due-at-zero')
      : t(`admin-rule-elapsed-summary-${settings.clockName.replace(/_/g, '-')}`, {
          duration: durationText(t, minutes),
        })
  const every = settings.repeat ? toMinutes(settings.every) : null
  return every == null ? when : `${when} ${t('admin-rule-elapsed-summary-repeat', { duration: durationText(t, every) })}`
}
