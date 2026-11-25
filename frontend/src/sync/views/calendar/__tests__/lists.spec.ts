import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick, type PropType } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import enUS from '../../../../../../i18n/locales/en-US/main.ftl?raw'
import type { CardData } from '@nosdesk/core/sync/views/types'
import { jobsByDay } from '@nosdesk/core/sync/views/calendar'
import AgendaList from '../AgendaList.vue'
import DayList from '../DayList.vue'

const TODAY = '2026-11-25'

function job(id: number, due: string | null): CardData {
  return {
    id,
    number: id,
    title: `Job ${id}`,
    workflow_state: { id: 2, name: 'Open', category: 'backlog', color: 'gray' },
    priority: 'none',
    due_date: due ? `${due}T00:00:00` : null,
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    last_activity_at: '2026-11-01T00:00:00Z',
  }
}

/** A row with the open control the lists focus, and nothing else. */
const RowStub = defineComponent({
  props: { card: { type: Object as PropType<CardData>, required: true } },
  setup: (props) => () => h('li', { 'data-row': props.card.id }, [h('button', { 'data-row-open': '' }, props.card.title)]),
})

function fluent() {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  return createFluentVue({ bundles: [bundle] })
}

describe('DayList', () => {
  it('moves the focus to the first new row when the last batch comes in', async () => {
    const jobs = Array.from({ length: 5 }, (_, i) => job(i + 1, null))
    const wrapper = mount(DayList, {
      attachTo: document.body,
      props: { day: { jobs, timed: 0 }, today: TODAY, locale: 'en-AU', empty: '', limit: 3 },
      global: { plugins: [fluent()], stubs: { DayJobRow: RowStub } },
    })
    expect(wrapper.findAll('[data-row]')).toHaveLength(3)
    const more = wrapper.get('[data-show-more]')
    expect(more.text()).toBe('Show 2 more')
    ;(more.element as HTMLElement).focus()
    await more.trigger('click')
    await flushPromises()
    expect(wrapper.findAll('[data-row]')).toHaveLength(5)
    expect(wrapper.find('[data-show-more]').exists()).toBe(false)
    expect(document.activeElement?.closest('[data-row]')?.getAttribute('data-row')).toBe('4')
    wrapper.unmount()
  })
})

describe('AgendaList', () => {
  const observed: Element[] = []
  beforeEach(() => {
    observed.length = 0
    vi.stubGlobal(
      'IntersectionObserver',
      class {
        observe(el: Element) {
          observed.push(el)
        }
        disconnect() {}
      },
    )
    Element.prototype.scrollTo = vi.fn()
  })
  afterEach(() => vi.unstubAllGlobals())

  it('watches its ends again when one opens with no change to the days shown', async () => {
    // Today and six days after: seven listed days, the whole window ahead.
    const days = ['2026-11-25', '2026-11-26', '2026-11-27', '2026-11-28', '2026-11-29', '2026-11-30', '2026-12-01']
    const cards = days.map((d, i) => job(i + 1, d))
    const props = {
      byDay: jobsByDay(cards, 'due_date', 'UTC'),
      overlaysByDay: new Map(),
      today: TODAY,
      locale: 'en-AU',
      timed: true,
      dropDay: null,
      draggedId: null,
    }
    const wrapper = mount(AgendaList, {
      props,
      global: { plugins: [fluent()], stubs: { DayList: true } },
    })
    await flushPromises()
    expect(wrapper.find('[data-agenda-end] p').exists()).toBe(true)
    const before = observed.length

    // A colleague dates a job just past the last listed day.
    await wrapper.setProps({ byDay: jobsByDay([...cards, job(99, '2026-12-03')], 'due_date', 'UTC') })
    await nextTick()
    expect(wrapper.findAll('section[data-day]')).toHaveLength(7)
    expect(wrapper.find('[data-agenda-end] p').exists()).toBe(false)
    expect(observed.length).toBeGreaterThan(before)
    expect(observed).toContain(wrapper.get('[data-agenda-end]').element)
    wrapper.unmount()
  })
})
