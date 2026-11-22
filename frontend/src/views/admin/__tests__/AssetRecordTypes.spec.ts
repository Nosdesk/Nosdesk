import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, type Component } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import { ConfigProvider, TooltipProvider } from 'reka-ui'
import type { AdminAssetRecordType, AssetRecordCatalogueEntry } from '@nosdesk/core/types/asset'
// The harness's globals (storage, matchMedia, ResizeObserver) are set up on import.
import '@/test/mountWithProviders'

const route = vi.hoisted(() => ({ params: {} as Record<string, string> }))
const push = vi.hoisted(() => vi.fn())
const types = vi.hoisted(() => ({
  list: vi.fn(),
  get: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  archive: vi.fn(),
  restore: vi.fn(),
  delete: vi.fn(),
}))
const records = vi.hoisted(() => ({ catalogue: vi.fn(), addFromCatalogue: vi.fn() }))

vi.mock('vue-router', async () => {
  const { defineComponent: define, h: render } = await import('vue')
  return {
    useRoute: () => route,
    useRouter: () => ({ push }),
    RouterLink: define({
      props: { to: { type: [String, Object], required: true } },
      setup: (props, { slots }) => () => {
        const to = props.to as string | { path: string; query?: Record<string, string> }
        const href =
          typeof to === 'string' ? to : `${to.path}?${new URLSearchParams(to.query).toString()}`
        return render('a', { href }, slots.default?.())
      },
    }),
  }
})
vi.mock('@/components/common/BackButton.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@nosdesk/core/stores/toast', () => ({
  useToastStore: () => ({ success: vi.fn(), error: vi.fn() }),
}))
vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
}))
vi.mock('@nosdesk/core/services/assetRecordTypeService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetRecordTypeService')>()),
  assetRecordTypeService: types,
}))
vi.mock('@nosdesk/core/services/assetRecordService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetRecordService')>()),
  assetRecordService: records,
}))
vi.mock('@nosdesk/core/services/assetKindsService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetKindsService')>()),
  assetKindsService: { list: async () => [] },
}))

import AssetRecordTypesView from '../AssetRecordTypesView.vue'
import AssetRecordTypeEditView from '../AssetRecordTypeEditView.vue'

function recordType(overrides: Partial<AdminAssetRecordType> = {}): AdminAssetRecordType {
  return {
    id: 7,
    slug: 'calibration',
    label: 'Calibration',
    description: null,
    icon: null,
    color: null,
    field_schema: { type: 'object', properties: {} },
    field_order: [],
    core_fields: {},
    outcomes: [],
    applies_to_kinds: null,
    opens_with_status: null,
    is_builtin: false,
    sort_order: 100,
    archived_at: null,
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    created_by: null,
    record_count: 0,
    ...overrides,
  }
}

const REPAIR_ENTRY: AssetRecordCatalogueEntry = {
  slug: 'repair',
  icon: 'settings',
  sort_order: 10,
  opens_with_status: 'in_repair',
  field_schema: { type: 'object', properties: {} },
  field_order: [],
  core_fields: {},
  outcomes: [{ key: 'repaired', category: 'done' }],
  added: true,
  type: null,
}

let wrapper: VueWrapper | null = null

/**
 * Mounts under the app's providers and a fresh query cache, so a remount
 * reads from the service again as a reload would. Messages resolve to their
 * ids unless `messages` gives them text.
 */
async function mountView(component: Component, messages = ''): Promise<VueWrapper> {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  if (messages) bundle.addResource(new FluentResource(messages))
  const Host = defineComponent({
    setup: () => () =>
      h(ConfigProvider, {}, () => h(TooltipProvider, { delayDuration: 0 }, () => h(component))),
  })
  wrapper = mount(Host, {
    attachTo: document.body,
    global: {
      plugins: [createFluentVue({ bundles: [bundle] }), createPinia(), [PiniaColada, {}] as never],
    },
  })
  await flushPromises()
  return wrapper
}

async function openType(id: number, messages = ''): Promise<VueWrapper> {
  route.params = { id: String(id) }
  return mountView(AssetRecordTypeEditView, messages)
}

function unmount(): void {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
}

const button = (label: string) =>
  Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find((b) =>
    b.textContent?.includes(label),
  )

/** The field builder's names, top to bottom. */
const fieldNames = (w: VueWrapper) =>
  w
    .findAll('input[placeholder="asset-kind-attribute-row-name-placeholder"]')
    .map((i) => (i.element as HTMLInputElement).value)

beforeEach(() => {
  records.catalogue.mockResolvedValue([REPAIR_ENTRY])
})

afterEach(() => {
  unmount()
  vi.clearAllMocks()
})

describe('AssetRecordTypesView', () => {
  it('lists types with record counts, hides archived ones until asked, and offers built-ins not yet added', async () => {
    types.list.mockResolvedValue([
      recordType({ id: 1, slug: 'repair', label: null, is_builtin: true, record_count: 12 }),
      recordType({ id: 2, slug: 'calibration', label: 'Calibration', record_count: 0 }),
      recordType({ id: 3, slug: 'audit', label: 'Audit', archived_at: '2026-11-02T00:00:00Z' }),
    ])
    records.catalogue.mockResolvedValue([
      REPAIR_ENTRY,
      {
        ...REPAIR_ENTRY,
        slug: 'inspection',
        outcomes: [{ key: 'passed', category: 'done' }],
        added: false,
      },
    ])
    records.addFromCatalogue.mockResolvedValue(recordType({ slug: 'inspection', label: null }))
    const w = await mountView(AssetRecordTypesView)

    const rows = w.findAll('[data-test="record-type-row"]')
    expect(rows.map((r) => r.text())).toEqual([
      expect.stringContaining('asset-record-type-repair'),
      expect.stringContaining('Calibration'),
    ])
    expect(w.text()).not.toContain('Audit')

    const offered = w.findAll('[data-test="catalogue-entry"]')
    expect(offered).toHaveLength(1)
    expect(offered[0].text()).toContain('asset-record-type-inspection')
    expect(offered[0].text()).toContain('asset-record-outcome-inspection-passed')

    await w.get('[role="switch"]').trigger('click')
    expect(w.text()).toContain('Audit')

    types.list.mockClear()
    button('admin-asset-record-types-add')!.click()
    await flushPromises()
    expect(records.addFromCatalogue).toHaveBeenCalledWith('inspection')
    expect(types.list).toHaveBeenCalled()
  })
})

describe('AssetRecordTypeEditView', () => {
  it('keeps the field order through a save and a reload', async () => {
    // jsonb hands the properties back sorted; field_order is the display order.
    const stored = {
      type: 'object',
      properties: {
        fault: { type: 'string' },
        offsite: { type: 'boolean' },
        serial_received: { type: 'string' },
      },
    }
    types.get.mockResolvedValue(
      recordType({ field_schema: stored, field_order: ['serial_received', 'fault', 'offsite'] }),
    )
    types.update.mockImplementation(async (_id: number, body: { field_order?: string[] }) =>
      recordType({ field_schema: stored, field_order: body.field_order }),
    )

    let w = await openType(7)
    expect(fieldNames(w)).toEqual(['serial_received', 'fault', 'offsite'])

    await w.findAll('button[aria-label="asset-kind-attribute-row-move-down"]')[0].trigger('click')
    expect(fieldNames(w)).toEqual(['fault', 'serial_received', 'offsite'])

    await w.get('form').trigger('submit')
    await flushPromises()
    // A reorder sends the order alone, so it runs no check of existing records.
    expect(types.update).toHaveBeenCalledWith(
      7,
      { field_order: ['fault', 'serial_received', 'offsite'] },
      false,
    )
    expect(push).toHaveBeenCalledWith({ name: 'admin-asset-record-types' })

    unmount()
    types.get.mockResolvedValue(
      recordType({ field_schema: stored, field_order: ['fault', 'serial_received', 'offsite'] }),
    )
    w = await openType(7)
    expect(fieldNames(w)).toEqual(['fault', 'serial_received', 'offsite'])
  })

  it('keeps a field shape the builder cannot edit, and edits it as JSON', async () => {
    const stored = {
      type: 'object',
      properties: {
        notes: { type: 'string' },
        tasks_done: { type: 'array', items: { type: 'string' } },
      },
    }
    types.get.mockResolvedValue(
      recordType({ field_schema: stored, field_order: ['tasks_done', 'notes'] }),
    )
    types.update.mockResolvedValue(recordType())
    const w = await openType(7)
    expect(w.text()).toContain('asset-kind-attribute-row-raw-warning')

    // The builder writes the list field back as it was, so a reorder is all
    // that changes.
    await w.findAll('button[aria-label="asset-kind-attribute-row-move-down"]')[0].trigger('click')
    expect(fieldNames(w)).toEqual(['notes', 'tasks_done'])

    button('admin-asset-kinds-view-json')!.click()
    await flushPromises()
    const text = w.get('textarea')
    expect(JSON.parse((text.element as HTMLTextAreaElement).value)).toEqual(stored)
    await text.setValue(
      JSON.stringify({
        type: 'object',
        properties: {
          tasks_done: { type: 'array', items: { type: 'string' }, maxItems: 20 },
          notes: { type: 'string' },
        },
      }),
    )
    await w.get('form').trigger('submit')
    await flushPromises()
    expect(types.update).toHaveBeenCalledWith(
      7,
      {
        field_schema: {
          type: 'object',
          properties: {
            tasks_done: { type: 'array', items: { type: 'string' }, maxItems: 20 },
            notes: { type: 'string' },
          },
        },
        field_order: ['tasks_done', 'notes'],
      },
      false,
    )
  })

  it('saves a built-in whose outcomes read the catalogue even when the catalogue fails to load', async () => {
    records.catalogue.mockRejectedValue(new Error('offline'))
    const repair = recordType({
      id: 3,
      slug: 'repair',
      label: null,
      is_builtin: true,
      outcomes: [{ key: 'repaired', category: 'done' }],
    })
    types.get.mockResolvedValue(repair)
    types.update.mockResolvedValue(repair)
    const w = await openType(3)
    await w.get('input[data-test="description-input"]').setValue('Work sent to the vendor')
    await w.get('form').trigger('submit')
    await flushPromises()
    expect(types.update).toHaveBeenCalledWith(3, { description: 'Work sent to the vendor' }, false)
  })

  it('names a built-in\'s own fields from the catalogue and says "field" on a record type', async () => {
    records.catalogue.mockResolvedValue([{ ...REPAIR_ENTRY, field_order: ['fault'] }])
    types.get.mockResolvedValue(
      recordType({
        id: 3,
        slug: 'repair',
        label: null,
        is_builtin: true,
        field_schema: {
          type: 'object',
          properties: { fault: { type: 'string' }, bay: { type: 'string' } },
        },
        field_order: ['fault', 'bay'],
      }),
    )
    let w = await openType(3)
    const rows = w.findAll('[data-test="attribute-title"]')
    expect(rows.map((r) => r.text())).toEqual(['asset-record-field-repair-fault'])
    expect(fieldNames(w)).toEqual(['fault', 'bay'])
    expect(w.text()).toContain('admin-asset-record-types-fields-add')
    expect(w.text()).not.toContain('asset-kind-attribute-editor-add')
    expect(w.find('button[aria-label="Clipboard check"]').exists()).toBe(true)

    unmount()
    types.get.mockResolvedValue(recordType())
    w = await openType(7)
    expect(w.text()).toContain('admin-asset-record-types-fields-empty-title')
    expect(w.text()).not.toContain('asset-kind-attribute-editor-empty-title')
  })

  it('takes the record count from an archive without fetching the type again', async () => {
    const type = recordType({ record_count: 4 })
    types.get.mockResolvedValue(type)
    types.archive.mockResolvedValue({ ...type, archived_at: '2026-11-20T00:00:00Z' })
    const w = await openType(7)
    expect(w.text()).not.toContain('admin-asset-record-types-archived-tag')

    button('admin-asset-record-types-archive')!.click()
    await flushPromises()
    expect(types.archive).toHaveBeenCalledWith(7)
    expect(w.text()).toContain('admin-asset-record-types-archived-tag')
    expect(button('admin-asset-record-types-restore')).toBeDefined()
    expect(types.get).toHaveBeenCalledTimes(1)
    expect(types.list).not.toHaveBeenCalled()
  })

  it('puts a renamed built-in back to its catalogue name', async () => {
    const renamed = recordType({ id: 3, slug: 'repair', label: 'Fix-it', is_builtin: true })
    types.get.mockResolvedValue(renamed)
    types.update.mockResolvedValue({ ...renamed, label: null })

    let w = await openType(3)
    const name = () => w.get<HTMLInputElement>('input[data-test="name-input"]').element
    expect(w.get('[data-test="page-title"]').text()).toBe('Fix-it')
    expect(name().value).toBe('Fix-it')
    expect(name().placeholder).toBe('asset-record-type-repair')

    button('admin-asset-record-types-reset-name')!.click()
    await flushPromises()
    expect(name().value).toBe('')
    expect(button('admin-asset-record-types-reset-name')).toBeUndefined()

    await w.get('form').trigger('submit')
    await flushPromises()
    expect(types.update).toHaveBeenCalledWith(3, { label: null }, false)

    // Reloaded, the type has no label of its own and reads the catalogue's.
    unmount()
    types.get.mockResolvedValue({ ...renamed, label: null })
    w = await openType(3)
    expect(w.get('[data-test="page-title"]').text()).toBe('asset-record-type-repair')
    expect(name().value).toBe('')

    unmount()
    types.list.mockResolvedValue([{ ...renamed, label: null }])
    w = await mountView(AssetRecordTypesView)
    expect(w.get('[data-test="record-type-row"]').text()).toContain('asset-record-type-repair')
    expect(w.text()).not.toContain('Fix-it')
  })

  it('saves an outcome change some records no longer fit only when told to save anyway', async () => {
    const type = recordType({
      outcomes: [
        { key: 'passed', category: 'done', label: 'Passed' },
        { key: 'failed', category: 'not_done', label: 'Failed' },
      ],
      record_count: 9,
    })
    types.get.mockResolvedValue(type)
    types.update
      .mockRejectedValueOnce({
        response: {
          status: 409,
          data: {
            code: 'RECORDS_WOULD_NOT_FIT',
            error: '3 existing record(s) of this type would no longer fit.',
            invalid_count: 3,
            sample: [
              {
                id: 51,
                asset_id: 42,
                asset_name: 'Camera',
                asset_tag: '14',
                error: 'outcome `failed` is no longer one of the type\'s',
              },
              { id: 52, asset_id: 43, asset_name: 'Tripod', asset_tag: null, error: 'outcome' },
            ],
          },
        },
      })
      .mockResolvedValueOnce({ ...type, outcomes: [type.outcomes[0]] })

    const w = await openType(
      7,
      [
        'admin-asset-record-types-conflict-heading = { $count } records would no longer fit',
        'admin-asset-record-types-conflict-asset-tagged = { $name } ({ $tag })',
      ].join('\n'),
    )
    await w.findAll('button[aria-label="admin-asset-record-types-outcome-remove"]')[1].trigger('click')
    await w.get('form').trigger('submit')
    await flushPromises()

    const expected = { outcomes: [{ key: 'passed', category: 'done', label: 'Passed' }] }
    expect(types.update).toHaveBeenNthCalledWith(1, 7, expected, false)
    const conflict = w.get('[data-test="records-conflict"]')
    expect(conflict.text()).toContain('3 records would no longer fit')
    // Each names its equipment and links to the record.
    const links = conflict.findAll('a')
    expect(links.map((a) => a.text())).toEqual(['Camera (14)', 'Tripod'])
    expect(links[0].attributes('href')).toBe('/assets/42?record=51')
    expect(push).not.toHaveBeenCalled()
    expect(w.find('[data-test="save"]').exists()).toBe(false)

    await w.get('[data-test="force-save"]').trigger('click')
    await flushPromises()
    expect(types.update).toHaveBeenNthCalledWith(2, 7, expected, true)
    expect(push).toHaveBeenCalledWith({ name: 'admin-asset-record-types' })
  })

  it('drops the refusal once the form changes again', async () => {
    types.get.mockResolvedValue(
      recordType({ outcomes: [{ key: 'passed', category: 'done', label: 'Passed' }] }),
    )
    types.update.mockRejectedValueOnce({
      response: { status: 409, data: { code: 'RECORDS_WOULD_NOT_FIT', invalid_count: 1, sample: [] } },
    })
    const w = await openType(7)
    await w.get('button[aria-label="admin-asset-record-types-outcome-remove"]').trigger('click')
    await w.get('form').trigger('submit')
    await flushPromises()
    expect(w.find('[data-test="records-conflict"]').exists()).toBe(true)

    await w.get('input[data-test="name-input"]').setValue('Calibration check')
    expect(w.find('[data-test="records-conflict"]').exists()).toBe(false)
    expect(w.find('[data-test="save"]').exists()).toBe(true)
  })

  it('creates a type with its outcomes, shared fields and kinds', async () => {
    route.params = { id: 'new' }
    types.create.mockResolvedValue(recordType())
    const w = await mountView(AssetRecordTypeEditView)

    await w.get('input[data-test="name-input"]').setValue('Calibration')
    button('admin-asset-record-types-outcome-add')!.click()
    await flushPromises()
    await w.get('[data-test="outcome-row"] input').setValue('In tolerance')
    await w.get('form').trigger('submit')
    await flushPromises()

    expect(types.create).toHaveBeenCalledWith({
      slug: 'calibration',
      label: 'Calibration',
      description: null,
      icon: null,
      color: null,
      field_schema: { type: 'object', properties: {} },
      field_order: [],
      core_fields: {},
      outcomes: [{ key: 'in_tolerance', category: 'done', label: 'In tolerance' }],
      applies_to_kinds: null,
    })
  })
})
