import { describe, expect, it } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { ticketSourceLabelKey } from './ticketSource'
import enUS from '../../../i18n/locales/en-US/main.ftl?raw'
import frFR from '../../../i18n/locales/fr-FR/main.ftl?raw'
import nlNL from '../../../i18n/locales/nl-NL/main.ftl?raw'

describe('ticketSourceLabelKey', () => {
  it('names every mail provider, hosted ones included', () => {
    for (const provider of ['email_imap', 'email_smtp', 'email_forward', 'email_managed']) {
      expect(ticketSourceLabelKey(provider), provider).not.toBeNull()
    }
    expect(ticketSourceLabelKey('slack')).toBe('ticket-detail-source-slack')
    expect(ticketSourceLabelKey('webhook')).toBeNull()
  })

  it('has every key it names in each catalogue', () => {
    for (const ftl of [enUS, frFR, nlNL]) {
      const bundle = new FluentBundle('en-US')
      bundle.addResource(new FluentResource(ftl))
      for (const provider of ['email_imap', 'email_forward', 'email_managed', 'slack', 'teams']) {
        const key = ticketSourceLabelKey(provider)!
        expect(bundle.hasMessage(key), key).toBe(true)
      }
    }
  })
})
