import { describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import enUS from '../../../../i18n/locales/en-US/main.ftl?raw'
import frFR from '../../../../i18n/locales/fr-FR/main.ftl?raw'
import nlNL from '../../../../i18n/locales/nl-NL/main.ftl?raw'
import type { RuleAction } from '@nosdesk/core/types/rule'

vi.mock('@/services/userService', () => ({ default: { getUsersBatch: async () => [] } }))
// Name lookups load nothing here, so a named person reads as unknown.
vi.mock('@nosdesk/core/services/groupService', () => ({ groupService: { getGroups: async () => [] } }))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ load: async () => [], findById: () => undefined }),
}))
const TAGS: Record<number, string> = { 3: 'printer', 4: 'vip' }
vi.mock('@nosdesk/core/stores/tags', () => ({
  useTagsStore: () => ({ findById: (id: number) => (TAGS[id] ? { id, name: TAGS[id] } : null) }),
}))

vi.mock('@/composables/useAssetTimeline', async () => {
  const { computed } = await import('vue')
  return {
    useAssetRecordTypes: () => ({
      types: computed(() => [
        { id: 1, slug: 'repair', label: null, outcomes: [{ key: 'repaired', category: 'done', label: null }] },
        { id: 2, slug: 'calibration', label: 'Calibration', outcomes: [{ key: 'passed', category: 'done', label: 'In tolerance' }] },
      ]),
    }),
  }
})

import { useRuleStepText } from '@/composables/useRuleStepText'

const CATALOGUES = { 'en-US': enUS, 'fr-FR': frFR, 'nl-NL': nlNL }

/** The step text helpers, translating with `locale`'s real catalogue. */
function stepText(locale: keyof typeof CATALOGUES) {
  const bundle = new FluentBundle(locale, { useIsolating: false })
  bundle.addResource(new FluentResource(CATALOGUES[locale]))
  let text!: ReturnType<typeof useRuleStepText>
  mount(
    defineComponent({
      setup() {
        text = useRuleStepText({ enabled: () => false, userUuids: () => [] })
        return () => null
      },
    }),
    { global: { plugins: [createFluentVue({ bundles: [bundle] }), createPinia(), PiniaColada] } },
  )
  return text
}

const addTags = (config: Record<string, unknown>): RuleAction => ({ kind: 'add_tags', config }) as RuleAction

describe('useRuleStepText', () => {
  it('quotes each tag name a step will add or remove', () => {
    const { planned } = stepText('en-US')
    expect(planned(addTags({ tag_ids: [3] }))).toBe('Add the tag “printer”')
    expect(planned(addTags({ tag_ids: [], tag_names: ['escalated', 'vip'] }))).toBe(
      'Add the tags “escalated”, “vip”',
    )
    expect(planned({ kind: 'remove_tags', config: { tag_ids: [3, 4] } } as RuleAction)).toBe(
      'Remove the tags “printer”, “vip”',
    )
  })

  it('quotes each tag name a run added or removed', () => {
    const { done } = stepText('en-US')
    expect(done({ kind: 'add_tags', tag_ids_added: [4] })).toBe('Added the tag “vip”')
    expect(done({ kind: 'remove_tags', tag_ids_removed: [3, 4] })).toBe('Removed the tags “printer”, “vip”')
  })

  it("uses the locale's quotation marks", () => {
    expect(stepText('fr-FR').planned(addTags({ tag_ids: [3] }))).toBe('Ajouter l\'étiquette « printer »')
    expect(stepText('nl-NL').planned(addTags({ tag_ids: [3] }))).toBe('De tag ‘printer’ toevoegen')
  })

  it('leaves a tag it cannot name unquoted and counted', () => {
    expect(stepText('en-US').planned(addTags({ tag_ids: [99] }))).toBe('Add a tag')
  })

  it('says what a record step will do, naming the type and outcome', () => {
    const { planned } = stepText('en-US')
    const step = (kind: RuleAction['kind'], config: Record<string, unknown>) => planned({ kind, config })
    expect(step('open_asset_record', { record_type: 'repair' })).toBe(
      'Open a record of type “Repair” on each linked asset',
    )
    expect(step('open_asset_record', { record_type: 'gone' })).toBe('Open a record on each linked asset')
    expect(step('complete_asset_records', { record_type: 'repair', outcome: 'repaired' })).toBe(
      "Complete the ticket's open records of type “Repair” as “Repaired”",
    )
    expect(step('complete_asset_records', { record_type: 'repair', outcome: 'repaired', create_if_none: true })).toBe(
      "Complete the ticket's records of type “Repair” as “Repaired”, writing one on each linked asset that has none",
    )
    expect(step('complete_asset_records', { record_type: null, outcome: 'passed' })).toBe(
      "Complete the ticket's open records as “In tolerance”",
    )
    expect(step('complete_asset_records', { record_type: 'repair', outcome: 'passed' })).toBe(
      "Complete the ticket's open records",
    )
  })

  it('says what a record step did', () => {
    const { done } = stepText('en-US')
    expect(done({ kind: 'open_asset_record', asset_ids: [], already_open_asset_ids: [] })).toBe(
      'Found no linked assets',
    )
    expect(done({ kind: 'open_asset_record', asset_ids: [], already_open_asset_ids: [4] })).toBe(
      'Opened no new records',
    )
    expect(done({ kind: 'open_asset_record', asset_ids: [4, 5] })).toBe('Opened 2 records')
    expect(done({ kind: 'complete_asset_records', completed_record_ids: [], created_asset_ids: [] })).toBe(
      'Found no open records to complete',
    )
    expect(done({ kind: 'complete_asset_records', completed_record_ids: [7], created_asset_ids: [] })).toBe(
      'Completed one record',
    )
    expect(done({ kind: 'complete_asset_records', completed_record_ids: [], created_asset_ids: [4] })).toBe(
      'Wrote one completed record where there was none',
    )
    expect(done({ kind: 'complete_asset_records', completed_record_ids: [7, 8], created_asset_ids: [4] })).toBe(
      'Completed 2 records and wrote one where there was none',
    )
  })

  it('words the record steps in French and Dutch', () => {
    const entry = { kind: 'complete_asset_records', completed_record_ids: [7, 8], created_asset_ids: [4] }
    expect(stepText('fr-FR').done(entry)).toBe(
      "2 interventions terminées et une créée là où il n'y en avait aucune",
    )
    expect(stepText('nl-NL').done(entry)).toBe('2 registraties afgerond en één geschreven waar er geen was')
    expect(stepText('fr-FR').done({ kind: 'open_asset_record', asset_ids: [], already_open_asset_ids: [4] })).toBe(
      'Aucune nouvelle intervention ouverte',
    )
  })

  it('says what an asset step will do', () => {
    const { planned } = stepText('en-US')
    const step = (kind: RuleAction['kind'], config: Record<string, unknown> = {}) => planned({ kind, config })
    expect(step('assign_assets_to_requester')).toBe('Assign the linked assets to the requester')
    expect(step('lend_assets')).toBe('Lend the linked assets to the requester')
    expect(step('lend_assets', { due_in_days: 14 })).toBe('Lend the linked assets to the requester for 14 days')
    expect(step('return_requester_assets')).toBe("Take back the requester's loans and assets")
  })

  it('says what an asset step did, and when it found nothing', () => {
    const { done } = stepText('en-US')
    expect(done({ kind: 'assign_assets_to_requester', asset_ids: [] })).toBe('Found no linked assets')
    expect(done({ kind: 'assign_assets_to_requester', asset_ids: [1, 2], unchanged_asset_ids: [] })).toBe(
      'Assigned 2 assets to the requester',
    )
    expect(done({ kind: 'assign_assets_to_requester', asset_ids: [], unchanged_asset_ids: [1] })).toBe(
      'Left the linked assets as they were',
    )
    expect(done({ kind: 'lend_assets', asset_ids: [], loan_ids: [] })).toBe('Found no linked assets')
    expect(done({ kind: 'lend_assets', asset_ids: [5], loan_ids: [9] })).toBe('Lent one asset to the requester')
    expect(done({ kind: 'return_requester_assets', loan_ids: [], cleared_asset_ids: [] })).toBe(
      'The requester had no loans or assets to take back',
    )
    expect(done({ kind: 'return_requester_assets', loan_ids: [1], cleared_asset_ids: [2, 3] })).toBe(
      'Took back one loan and 2 assets from the requester',
    )
  })

  it('reads the asset steps in French and Dutch', () => {
    const entry = { kind: 'return_requester_assets', loan_ids: [1, 2], cleared_asset_ids: [3] }
    expect(stepText('fr-FR').done(entry)).toBe('Récupéré auprès du demandeur : 2 prêts et 1 ressource')
    expect(stepText('nl-NL').done(entry)).toBe('Van de aanvrager teruggenomen: 2 uitleningen en één activum')
  })

  it('says who a notify step will email', () => {
    const { planned } = stepText('en-US')
    const notify = (config: Record<string, unknown>) => planned({ kind: 'notify', config })
    expect(notify({ recipient: 'requester', subject: 's', body: 'b' })).toBe('Email the requester')
    expect(notify({ recipient: 'user', user_uuid: 'u-ana', subject: 's', body: 'b' })).toBe('Email a specific person')
  })

  it('says who a notify step emailed, or why it sent nothing', () => {
    const { done } = stepText('en-US')
    expect(done({ kind: 'notify', recipient: 'requester' })).toBe('Emailed the requester')
    expect(done({ kind: 'notify', recipient: 'user', user_uuid: 'u-ana' })).toBe('Emailed a specific person')
    expect(done({ kind: 'notify', recipient: 'requester', skipped: 'no_requester' })).toBe(
      'Sent no email, as the ticket has no requester',
    )
    expect(done({ kind: 'notify', recipient: 'requester', skipped: 'no_email' })).toBe(
      'Sent no email, as the requester has no email address',
    )
  })

  it('reads the notify step in French and Dutch', () => {
    const step = { kind: 'notify', config: { recipient: 'requester', subject: 's', body: 'b' } } as RuleAction
    expect(stepText('fr-FR').planned(step)).toBe('Envoyer un e-mail au demandeur')
    expect(stepText('nl-NL').planned(step)).toBe('De aanvrager e-mailen')
    expect(stepText('nl-NL').done({ kind: 'notify', recipient: 'requester' })).toBe('E-mail verstuurd naar de aanvrager')
  })
})
