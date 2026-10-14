import { beforeEach, describe, expect, it } from 'vitest'
import { nextTick, ref } from 'vue'
import { useDataTableColumns, type DataTableColumnLike } from '@/composables/useDataTableColumns'

// Columns derived from data that loads after setup (asset kind attributes)
// must not reshuffle or reveal themselves when they arrive, and a column the
// user picked keeps its place and visibility across reloads.

const CORE: DataTableColumnLike[] = [
  { field: 'name', label: 'Name' },
  { field: 'serial', label: 'Serial' },
  { field: 'status', label: 'Status' },
]
const ATTRS: DataTableColumnLike[] = [
  { field: 'attr:bay', label: 'Bay', defaultHidden: true },
  { field: 'attr:colour', label: 'Colour', defaultHidden: true },
]
const NS = 'spec'

function setup() {
  const columns = ref<readonly DataTableColumnLike[]>(CORE)
  const table = useDataTableColumns({
    columns,
    storageNamespace: NS,
    getViewId: () => 'default',
    pinnedIds: ['name'],
    retainUnknownIds: (id) => id.startsWith('attr:'),
  })
  return { columns, table }
}

const visible = (table: ReturnType<typeof setup>['table']) =>
  table.visible.value.map((c) => c.field)

beforeEach(() => localStorage.clear())

describe('useDataTableColumns with late columns', () => {
  it('keeps late default-hidden columns hidden, with or without a stored layout', async () => {
    const fresh = setup()
    fresh.columns.value = [...CORE, ...ATTRS]
    await nextTick()
    expect(visible(fresh.table)).toEqual(['name', 'serial', 'status'])

    // A stored layout (any reorder or toggle persists one) predates them.
    localStorage.setItem(`${NS}-columns-order:default`, JSON.stringify(['name', 'status', 'serial']))
    localStorage.setItem(`${NS}-columns-hidden:default`, JSON.stringify([]))
    const stored = setup()
    stored.columns.value = [...CORE, ...ATTRS]
    await nextTick()
    expect(visible(stored.table)).toEqual(['name', 'status', 'serial'])
  })

  it('keeps a picked late column where the user put it across a reload', async () => {
    const first = setup()
    first.columns.value = [...CORE, ...ATTRS]
    await nextTick()
    first.table.toggleVisible('attr:colour')
    first.table.applyLayout(
      ['name', 'attr:colour', 'serial', 'status', 'attr:bay'],
      first.table.captureLayout().hidden,
    )
    expect(visible(first.table)).toEqual(['name', 'attr:colour', 'serial', 'status'])

    // Reload: the registry starts without the attributes, then they arrive.
    const second = setup()
    expect(visible(second.table)).toEqual(['name', 'serial', 'status'])
    second.columns.value = [...CORE, ...ATTRS]
    await nextTick()
    expect(visible(second.table)).toEqual(['name', 'attr:colour', 'serial', 'status'])
  })

  it('round-trips attribute columns through a saved layout', async () => {
    const { columns, table } = setup()
    columns.value = [...CORE, ...ATTRS]
    await nextTick()
    table.toggleVisible('attr:bay')
    const saved = table.captureLayout()
    table.reset()
    expect(visible(table)).toEqual(['name', 'serial', 'status'])
    table.applyLayout(saved.order, saved.hidden, saved.widths)
    expect(visible(table)).toEqual(['name', 'serial', 'status', 'attr:bay'])
  })
})
