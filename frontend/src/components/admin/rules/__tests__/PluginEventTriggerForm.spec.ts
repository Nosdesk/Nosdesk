import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, ref } from 'vue'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { Plugin } from '@nosdesk/core/types/plugin'

vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
}))
const listPlugins = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/services/pluginService', () => ({ default: { listPlugins } }))
// The pickers, reduced to what they were given and what they send back.
vi.mock('@/components/common/BaseDropdown.vue', () => ({
  default: {
    name: 'BaseDropdownStub',
    props: ['options', 'modelValue', 'label', 'disabled', 'placeholder', 'size'],
    emits: ['update:modelValue'],
    template: '<div data-test="dropdown"></div>',
  },
}))

import PluginEventTriggerForm from '../PluginEventTriggerForm.vue'
import { readTriggerConfig, triggerConfigFor, triggerProblem } from '../triggerConfig'

const plugin = (uuid: string, name: string, events: string[], state = 'installed') =>
  ({ uuid, name, display_name: name, state, manifest: { events } }) as unknown as Plugin

const GITHUB = '0190a5b2-0000-7000-8000-0000000000a1'
const JIRA = '0190a5b2-0000-7000-8000-0000000000a2'
const MAPS = '0190a5b2-0000-7000-8000-0000000000a3'
const OLD = '0190a5b2-0000-7000-8000-0000000000a4'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  listPlugins.mockReset()
})

async function mountForm(start: { pluginUuid?: string; eventType?: string } = {}) {
  const pluginUuid = ref(start.pluginUuid ?? '')
  const eventType = ref(start.eventType ?? '')
  const Harness = defineComponent({
    setup: () => () =>
      h(PluginEventTriggerForm, {
        pluginUuid: pluginUuid.value,
        eventType: eventType.value,
        'onUpdate:pluginUuid': (v: string) => {
          pluginUuid.value = v
        },
        'onUpdate:eventType': (v: string) => {
          eventType.value = v
        },
      }),
  })
  wrapper = mountWithProviders(Harness, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  const dropdown = (label: string) =>
    wrapper!.findAllComponents({ name: 'BaseDropdownStub' }).find((d) => d.props('label') === label)!
  return { w: wrapper, pluginUuid, eventType, dropdown }
}

const values = (options: { value: string; label: string }[]) => options.map((o) => o.value)

describe('PluginEventTriggerForm', () => {
  it('offers the installed plugins that emit events, then the events of the one picked', async () => {
    listPlugins.mockResolvedValue([
      plugin(GITHUB, 'GitHub', ['github:pr_merged', 'github:issue_linked']),
      plugin(JIRA, 'Jira', ['jira:issue_moved'], 'disabled'),
      plugin(MAPS, 'Maps', []),
      plugin(OLD, 'Old', ['old:synced'], 'uninstalled'),
    ])
    const { pluginUuid, eventType, dropdown } = await mountForm()
    const plugins = dropdown('admin-rule-editor-plugin-label')
    expect(values(plugins.props('options'))).toEqual([GITHUB, JIRA])
    expect(plugins.props('options')[1].label).toBe('admin-rule-editor-plugin-not-running')
    expect(dropdown('admin-rule-editor-plugin-event-label').props('disabled')).toBe(true)

    // Two events: the admin picks one.
    plugins.vm.$emit('update:modelValue', GITHUB)
    await flushPromises()
    expect(pluginUuid.value).toBe(GITHUB)
    expect(eventType.value).toBe('')
    const events = dropdown('admin-rule-editor-plugin-event-label')
    expect(values(events.props('options'))).toEqual(['github:pr_merged', 'github:issue_linked'])
    events.vm.$emit('update:modelValue', 'github:issue_linked')
    await flushPromises()
    expect(eventType.value).toBe('github:issue_linked')

    // Another plugin's events are its own: it takes its only one.
    dropdown('admin-rule-editor-plugin-label').vm.$emit('update:modelValue', JIRA)
    await flushPromises()
    expect([pluginUuid.value, eventType.value]).toEqual([JIRA, 'jira:issue_moved'])
  })

  it('takes the only event of a plugin with one', async () => {
    listPlugins.mockResolvedValue([plugin(JIRA, 'Jira', ['jira:issue_moved'])])
    const { eventType, dropdown } = await mountForm({ pluginUuid: GITHUB, eventType: 'github:pr_merged' })
    dropdown('admin-rule-editor-plugin-label').vm.$emit('update:modelValue', JIRA)
    await flushPromises()
    expect(eventType.value).toBe('jira:issue_moved')
  })

  it('keeps a saved plugin and event that are gone, so the rule still reads', async () => {
    listPlugins.mockResolvedValue([plugin(JIRA, 'Jira', ['jira:issue_moved'])])
    const { dropdown } = await mountForm({ pluginUuid: OLD, eventType: 'old:synced' })
    const plugins = dropdown('admin-rule-editor-plugin-label').props('options')
    expect(plugins.at(-1)).toEqual({ value: OLD, label: 'admin-rule-editor-plugin-missing' })
    expect(values(dropdown('admin-rule-editor-plugin-event-label').props('options'))).toEqual(['old:synced'])
  })

  it('says so when no installed plugin emits events', async () => {
    listPlugins.mockResolvedValue([plugin(MAPS, 'Maps', [])])
    const { w } = await mountForm()
    expect(w.find('[data-test="plugin-event-none"]').exists()).toBe(true)
    expect(w.findAllComponents({ name: 'BaseDropdownStub' })).toHaveLength(0)
  })
})

describe('plugin event trigger settings', () => {
  it('round-trip through trigger_config and need a plugin and an event', () => {
    const settings = readTriggerConfig({ plugin_uuid: GITHUB, event_type: 'github:issue_linked' })
    expect(settings).toMatchObject({ pluginUuid: GITHUB, pluginEvent: 'github:issue_linked' })
    expect(triggerConfigFor('plugin_event', settings)).toEqual({ plugin_uuid: GITHUB, event_type: 'github:issue_linked' })
    expect(triggerProblem('plugin_event', settings)).toBeNull()

    const unpicked = readTriggerConfig({})
    expect(triggerConfigFor('plugin_event', unpicked)).toEqual({})
    expect(triggerProblem('plugin_event', unpicked)).toBe('admin-rule-editor-plugin-incomplete')
    expect(triggerProblem('plugin_event', { ...unpicked, pluginUuid: GITHUB })).toBe(
      'admin-rule-editor-plugin-incomplete',
    )
    // Other triggers carry no plugin.
    expect(triggerConfigFor('ticket_created', settings)).toEqual({})
    expect(triggerProblem('ticket_created', unpicked)).toBeNull()
  })
})
