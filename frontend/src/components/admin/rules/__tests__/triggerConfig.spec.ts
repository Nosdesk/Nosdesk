import { describe, expect, it } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import enUS from '../../../../../../i18n/locales/en-US/main.ftl?raw'
import frFR from '../../../../../../i18n/locales/fr-FR/main.ftl?raw'
import nlNL from '../../../../../../i18n/locales/nl-NL/main.ftl?raw'
import type { RuleTriggerConfig } from '@nosdesk/core/types/rule'

import {
  ELAPSED_CLOCKS,
  elapsedSummary,
  readTriggerConfig,
  splitMinutes,
  triggerConfigFor,
  triggerProblem,
  type TriggerSettings,
} from '../triggerConfig'

const CATALOGUES = { 'en-US': enUS, 'fr-FR': frFR, 'nl-NL': nlNL }

/** Translates with `locale`'s real catalogue, failing on any missing key or
 *  formatting error. */
function translator(locale: keyof typeof CATALOGUES) {
  const bundle = new FluentBundle(locale, { useIsolating: false })
  const parseErrors = bundle.addResource(new FluentResource(CATALOGUES[locale]))
  expect(parseErrors).toEqual([])
  return (key: string, args?: Record<string, string | number>) => {
    const message = bundle.getMessage(key)
    if (!message?.value) throw new Error(`${locale} has no ${key}`)
    const errors: Error[] = []
    const text = bundle.formatPattern(message.value, args, errors)
    if (errors.length > 0) throw errors[0]
    return text
  }
}

const roundTrip = (config: RuleTriggerConfig) => triggerConfigFor('time_elapsed', readTriggerConfig(config))
const time = (config: RuleTriggerConfig): TriggerSettings => readTriggerConfig(config)

describe('time-elapsed trigger settings', () => {
  it('saves what it read, in the shape the server saves', () => {
    for (const config of [
      { clock_name: 'awaiting_requester', minutes: 43200, repeat_every_minutes: null },
      { clock_name: 'since_created', minutes: 90, repeat_every_minutes: 120 },
      { clock_name: 'until_due_at', minutes: 0, repeat_every_minutes: null },
      { clock_name: 'since_last_agent_reply', minutes: 1, repeat_every_minutes: 1500 },
    ]) {
      expect(roundTrip(config)).toEqual(config)
    }
    // A config with no repeat key reads as no repeat.
    expect(roundTrip({ clock_name: 'since_assigned', minutes: 60 })).toEqual({
      clock_name: 'since_assigned',
      minutes: 60,
      repeat_every_minutes: null,
    })
  })

  it('reads a time in the largest unit that divides it exactly', () => {
    expect(splitMinutes(43200)).toEqual({ amount: 30, unit: 'days' })
    expect(splitMinutes(1440)).toEqual({ amount: 1, unit: 'days' })
    expect(splitMinutes(1500)).toEqual({ amount: 25, unit: 'hours' })
    expect(splitMinutes(60)).toEqual({ amount: 1, unit: 'hours' })
    expect(splitMinutes(90)).toEqual({ amount: 90, unit: 'minutes' })
    expect(splitMinutes(0)).toEqual({ amount: 0, unit: 'minutes' })
    expect(time({ clock_name: 'since_created', minutes: 4320 }).after).toEqual({ amount: 3, unit: 'days' })
  })

  it('saves an amount and unit as minutes', () => {
    const settings = time({ clock_name: 'since_created', minutes: 60 })
    settings.after = { amount: 60, unit: 'minutes' }
    expect(triggerConfigFor('time_elapsed', settings).minutes).toBe(60)
    settings.after = { amount: 2, unit: 'days' }
    expect(triggerConfigFor('time_elapsed', settings).minutes).toBe(2880)
  })

  it('repeats only while the repeat is on, keeping its time while off', () => {
    const settings = time({ clock_name: 'awaiting_requester', minutes: 1440, repeat_every_minutes: 180 })
    expect(settings.repeat).toBe(true)
    expect(settings.every).toEqual({ amount: 3, unit: 'hours' })
    settings.repeat = false
    expect(triggerConfigFor('time_elapsed', settings).repeat_every_minutes).toBeNull()
    settings.repeat = true
    expect(triggerConfigFor('time_elapsed', settings).repeat_every_minutes).toBe(180)

    // Off by default, offering a day once turned on.
    const fresh = time({})
    expect(fresh.repeat).toBe(false)
    fresh.repeat = true
    expect(triggerConfigFor('time_elapsed', fresh).repeat_every_minutes).toBe(1440)
  })

  it('reads a clock it does not offer as no clock', () => {
    expect(time({ clock_name: 'until_sla_due_at', minutes: 60 }).clockName).toBe('')
    expect(time({ clock_name: 'whenever', minutes: 60 }).clockName).toBe('')
    expect(triggerProblem('time_elapsed', time({ clock_name: 'until_sla_due_at', minutes: 60 }))).toBe(
      'admin-rule-editor-elapsed-clock-missing',
    )
  })

  it('takes the times the server takes', () => {
    const problem = (config: RuleTriggerConfig, change: Partial<TriggerSettings> = {}) =>
      triggerProblem('time_elapsed', { ...time(config), ...change })
    expect(problem({ clock_name: 'since_created', minutes: 1 })).toBeNull()
    expect(problem({ clock_name: 'since_created', minutes: 525600 })).toBeNull()
    expect(problem({})).toBe('admin-rule-editor-elapsed-clock-missing')
    expect(problem({ clock_name: 'since_created' })).toBe('admin-rule-editor-elapsed-after-missing')
    expect(problem({ clock_name: 'since_created', minutes: 0 })).toBe('admin-rule-editor-elapsed-after-range')
    expect(problem({ clock_name: 'since_created' }, { after: { amount: 366, unit: 'days' } })).toBe(
      'admin-rule-editor-elapsed-after-range',
    )
    // The due date clock counts down, so it can run right as the due day ends.
    expect(problem({ clock_name: 'until_due_at', minutes: 0 })).toBeNull()
    expect(problem({ clock_name: 'until_due_at' }, { after: { amount: 366, unit: 'days' } })).toBe(
      'admin-rule-editor-elapsed-before-range',
    )
    // A repeat runs at most hourly.
    expect(problem({ clock_name: 'since_created', minutes: 60, repeat_every_minutes: 60 })).toBeNull()
    expect(problem({ clock_name: 'since_created', minutes: 60 }, { repeat: true, every: { amount: 59, unit: 'minutes' } })).toBe(
      'admin-rule-editor-elapsed-repeat-range',
    )
    expect(problem({ clock_name: 'since_created', minutes: 60 }, { repeat: true, every: { amount: null, unit: 'hours' } })).toBe(
      'admin-rule-editor-elapsed-repeat-missing',
    )
    // A repeat that's off isn't checked.
    expect(problem({ clock_name: 'since_created', minutes: 60 }, { repeat: false, every: { amount: 1, unit: 'minutes' } })).toBeNull()
    // Other triggers don't read the clock.
    expect(triggerProblem('ticket_created', time({}))).toBeNull()
  })
})

describe('time-elapsed summary', () => {
  it('reads as a sentence', () => {
    const t = translator('en-US')
    const say = (config: RuleTriggerConfig) => elapsedSummary(t, time(config))
    expect(say({ clock_name: 'awaiting_requester', minutes: 4320 })).toBe(
      'When a ticket has waited 3 days on the requester.',
    )
    expect(say({ clock_name: 'since_created', minutes: 60 })).toBe('When a ticket is 1 hour old.')
    expect(say({ clock_name: 'until_due_at', minutes: 2880 })).toBe('When a ticket is due in 2 days.')
    expect(say({ clock_name: 'until_due_at', minutes: 0 })).toBe('When a ticket reaches the end of its due day.')
    expect(say({ clock_name: 'since_last_agent_reply', minutes: 90, repeat_every_minutes: 1440 })).toBe(
      'When staff last replied in public on a ticket 90 minutes ago. It repeats 1 day after each run while the clock keeps running.',
    )
    expect(say({ clock_name: 'since_created' })).toBeNull()
    expect(say({ minutes: 60 })).toBeNull()
  })

  it('has every clock, unit and repeat in each catalogue', () => {
    for (const locale of Object.keys(CATALOGUES) as (keyof typeof CATALOGUES)[]) {
      const t = translator(locale)
      for (const clock of ELAPSED_CLOCKS) {
        for (const minutes of [1, 120, 2880]) {
          const text = elapsedSummary(t, time({ clock_name: clock, minutes, repeat_every_minutes: 60 }))
          expect(text, `${locale} ${clock}`).toBeTruthy()
          expect(text).not.toMatch(/[{}]/)
        }
        const key = clock.replace(/_/g, '-')
        expect(t(`admin-rule-editor-elapsed-clock-${key}`)).toBeTruthy()
        expect(t(`admin-rule-editor-elapsed-clock-${key}-hint`)).toBeTruthy()
      }
      expect(elapsedSummary(t, time({ clock_name: 'until_due_at', minutes: 0 }))).toBeTruthy()
    }
  })
})
