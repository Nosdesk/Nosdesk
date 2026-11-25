import { afterEach, describe, expect, it, vi } from 'vitest'
import { h } from 'vue'
import { mount } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'

const columns = [
  { field: 'name', label: 'Name' },
  { field: 'status', label: 'Status' },
  { field: 'owner', label: 'Owner', responsive: 'md' as const },
  { field: 'created', label: 'Created', responsive: 'lg' as const },
]
const data = [{ id: 1, name: 'Parramatta office laptops for the finance team', status: 'Active', owner: 'Priya Nair', created: 'Mon' }]

async function render(width: number) {
  window.matchMedia ??= ((query: string) => ({
    matches: false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  })) as unknown as typeof window.matchMedia
  vi.resetModules()
  Object.defineProperty(window, 'innerWidth', { value: width, configurable: true })
  const { default: DataTable } = await import('../DataTable.vue')
  return mount(DataTable, {
    props: { columns, data, selectedItems: [] },
    slots: { 'cell-status': ({ value }: { value: string }) => h('span', { class: 'truncate' }, value) },
    global: { plugins: [createPinia(), createFluentVue({ bundles: [new FluentBundle('en-US')] })] },
  })
}

afterEach(() => Object.defineProperty(window, 'innerWidth', { value: 1024, configurable: true }))

describe('DataTable on a phone', () => {
  it('shows each row as a card: the first column as its title, the rest labelled, through the same cell slots', async () => {
    const w = await render(400)
    expect(w.find('.data-table-grid').exists()).toBe(false)
    const card = w.find('[data-table-card]')
    expect(card.text()).toContain('Parramatta office laptops for the finance team')
    expect(card.findAll('dt').map((d) => d.text())).toEqual(['Status', 'Owner'])
    expect(card.find('dd .truncate').text()).toBe('Active')
    await card.trigger('click')
    expect(w.emitted('row-click')?.[0]).toEqual([data[0]])
  })

  it('keeps the grid from a tablet up', async () => {
    const w = await render(1024)
    expect(w.find('.data-table-grid').exists()).toBe(true)
    expect(w.find('[data-table-card]').exists()).toBe(false)
  })
})
