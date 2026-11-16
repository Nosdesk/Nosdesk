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
vi.mock('@nosdesk/core/services/groupService', () => ({ groupService: { getGroups: async () => [] } }))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ load: async () => [], findById: () => undefined }),
}))
const TAGS: Record<number, string> = { 3: 'printer', 4: 'vip' }
vi.mock('@nosdesk/core/stores/tags', () => ({
  useTagsStore: () => ({ findById: (id: number) => (TAGS[id] ? { id, name: TAGS[id] } : null) }),
}))

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
})
