import { describe, expect, it } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import type { ImportJob, KeptAsset, RemovalPlan } from '@nosdesk/core/services/importService'

import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'
import {
  dialogMessage,
  keptGroups,
  previousValuesHelp,
  reasonLine,
  removalState,
  resultLine,
} from '../removeModel'

/** The real en-US catalogue, so the sentences are checked as written. */
const bundle = new FluentBundle('en-US', { useIsolating: false })
bundle.addResource(new FluentResource(enUS))
const t = (key: string, args?: Record<string, string | number>) => {
  const message = bundle.getMessage(key)
  if (!message?.value) throw new Error(`no message ${key}`)
  return bundle.formatPattern(message.value, args)
}

const date = (iso: string) => iso.slice(0, 10)
const list = (items: string[]) => new Intl.ListFormat('en-US', { type: 'conjunction' }).format(items)

const NOW = Date.parse('2026-10-20T00:00:00Z')

function job(overrides: Partial<ImportJob> = {}): ImportJob {
  return {
    id: 'job-1',
    job_type: 'assets',
    status: 'done',
    state: 'imported',
    pruned_at: null,
    filename: 'Cameras.xlsx',
    created_by: 'u-1',
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
    completed_at: '2026-10-15T00:00:00Z',
    summary: {
      row_count: 412,
      would_create: 407,
      would_update: 0,
      unchanged: 0,
      errors: [],
      errors_truncated: false,
      errors_total: 5,
      rows_with_errors: 5,
    },
    records_committed: 407,
    error_message: null,
    source: null,
    suggested_mapping: [],
    mapping: null,
    options: null,
    removal_closes_at: '2026-11-14T00:00:00Z',
    ...overrides,
  }
}

const edited = (n: number): KeptAsset[] =>
  Array.from({ length: n }, (_, i) => ({
    id: i,
    tag: `E-${i}`,
    name: `Edited ${i}`,
    reasons: [{ code: 'edited', at: '2026-11-03T00:00:00Z' }],
  }))
const withPhotos = (n: number): KeptAsset[] =>
  Array.from({ length: n }, (_, i) => ({
    id: 100 + i,
    name: `Photo ${i}`,
    reasons: [
      { code: 'media', photos: 2, files: 0 },
      { code: 'edited', at: '2026-11-03T00:00:00Z' },
    ],
  }))
const linked = (n: number): KeptAsset[] =>
  Array.from({ length: n }, (_, i) => ({
    id: 200 + i,
    name: `Linked ${i}`,
    reasons: [{ code: 'ticket', ticket_id: 412 + i }],
  }))

function plan(overrides: Partial<RemovalPlan> = {}): RemovalPlan {
  return {
    created: 407,
    deletable: 398,
    gone: 0,
    kept: [...linked(2), ...edited(4), ...withPhotos(3)],
    catalog: { manufacturers: 1, models: 9, groups: 0 },
    closes_at: '2026-11-14T00:00:00Z',
    ...overrides,
  }
}

describe('removalState', () => {
  it('offers removal on an asset import that created assets, inside the window', () => {
    expect(removalState(job(), NOW)).toBe('offer')
  })

  it('says the window closed after 30 days or once the details are pruned', () => {
    expect(removalState(job(), Date.parse('2026-11-14T00:00:01Z'))).toBe('closed')
    expect(removalState(job({ pruned_at: '2026-11-15T00:00:00Z' }), NOW)).toBe('closed')
  })

  it('shows the result once it ran, even after the window', () => {
    const removed = job({
      removed_at: '2026-10-16T00:00:00Z',
      remove_result: { created: 407, deleted: 398, gone: 0, deleted_ids: [], kept: [] },
    })
    expect(removalState(removed, NOW)).toBe('removed')
    expect(removalState(removed, Date.parse('2027-01-01T00:00:00Z'))).toBe('removed')
  })

  it('offers nothing for users, tickets, drafts or an import that created nothing', () => {
    expect(removalState(job({ job_type: 'users', removal_closes_at: null }), NOW)).toBeNull()
    expect(removalState(job({ job_type: 'tickets', removal_closes_at: null }), NOW)).toBeNull()
    expect(removalState(job({ state: 'draft', status: 'dry_run_done' }), NOW)).toBeNull()
    const updatesOnly = job()
    updatesOnly.summary = { ...updatesOnly.summary!, would_create: 0, would_update: 180 }
    expect(removalState(updatesOnly, NOW)).toBeNull()
  })
})

describe('dialogMessage', () => {
  it('reads as the design writes it, with the webhook line when a webhook hears about assets', () => {
    expect(dialogMessage(t, plan(), { webhooks: true, list })).toBe(
      'This import created 407 assets. 398 have not been changed since and will be deleted. ' +
        '9 have been changed since and will be kept: 4 edited, 3 with photos, 2 linked to tickets. ' +
        '1 manufacturer and 9 models created by this import will be removed if nothing else uses them. ' +
        'Webhooks will receive 398 asset.deleted events.',
    )
  })

  it('leaves the webhook line out when no webhook hears about assets', () => {
    expect(dialogMessage(t, plan(), { webhooks: false, list })).not.toContain('Webhooks')
  })

  it('has no kept or catalog sentence when everything goes and nothing was created', () => {
    expect(
      dialogMessage(
        t,
        plan({ created: 8, deletable: 8, kept: [], catalog: { manufacturers: 0, models: 0, groups: 0 } }),
        { webhooks: false, list },
      ),
    ).toBe('This import created 8 assets. 8 have not been changed since and will be deleted.')
  })

  it('counts assets already deleted, and drops the delete and catalog lines when nothing goes', () => {
    expect(
      dialogMessage(t, plan({ created: 3, deletable: 0, gone: 1, kept: linked(1).concat(edited(1)) }), {
        webhooks: true,
        list,
      }),
    ).toBe(
      'This import created 3 assets. 2 have been changed since and will be kept: 1 linked to a ticket, 1 edited. ' +
        '1 was already deleted.',
    )
  })

  it('says only that an import that created no assets has nothing to remove', () => {
    expect(
      dialogMessage(t, plan({ created: 0, deletable: 0, gone: 0, kept: [] }), { webhooks: true, list }),
    ).toBe('This import created no assets, so there is nothing to remove.')
  })

  it('says every asset the import created was already deleted', () => {
    expect(
      dialogMessage(t, plan({ created: 2, deletable: 0, gone: 2, kept: [] }), { webhooks: true, list }),
    ).toBe('This import created 2 assets. 2 were already deleted.')
  })

  it('groups the kept assets by their first reason, largest first', () => {
    expect(keptGroups(t, [...linked(1), ...edited(2)])).toBe('2 edited, 1 linked to a ticket')
  })
})

describe('reasonLine', () => {
  it('words every reason', () => {
    const lines = [
      reasonLine(t, { code: 'edited', at: '2026-11-03T00:00:00Z', by_name: 'Sam Okafor' }, date),
      reasonLine(t, { code: 'edited', at: '2026-11-03T00:00:00Z' }, date),
      reasonLine(t, { code: 'status_changed' }, date),
      reasonLine(t, { code: 'loan' }, date),
      reasonLine(t, { code: 'disposed' }, date),
      reasonLine(t, { code: 'media', photos: 2, files: 1 }, date),
      reasonLine(t, { code: 'media', photos: 0, files: 1 }, date),
      reasonLine(t, { code: 'audited', at: '2026-11-02T00:00:00Z' }, date),
      reasonLine(t, { code: 'usage' }, date),
      reasonLine(t, { code: 'group_added' }, date),
      reasonLine(t, { code: 'ticket', ticket_id: 1412 }, date),
      reasonLine(t, { code: 'directory' }, date),
      reasonLine(t, { code: 'synced', source: 'intune' }, date),
      reasonLine(t, { code: 'synced', source: 'jamf' }, date),
      reasonLine(t, { code: 'referenced', asset_id: 9, name: 'Tripod kit 4' }, date),
    ]
    expect(lines).toEqual([
      'Edited on 2026-11-03 by Sam Okafor',
      'Edited on 2026-11-03',
      'Status changed since',
      'Has a loan record',
      'Disposed',
      'Has 2 photos',
      'Has 1 file',
      'Audited on 2026-11-02',
      'Usage logged',
      'Added to a group since',
      'Linked to ticket #1412',
      'In a directory group',
      'Now synced from Intune',
      'Now synced from an external source',
      "Referenced by 'Tripod kit 4'",
    ])
  })
})

describe('resultLine', () => {
  const removed = (kept: KeptAsset[], name: string | null = 'Priya Nair') =>
    job({
      removed_at: '2026-10-15T00:00:00Z',
      removed_by_name: name,
      remove_result: { created: 407, deleted: 398, gone: 0, deleted_ids: [], kept },
    })

  it('says how many went, when, by whom and how many stayed', () => {
    expect(resultLine(t, removed(edited(9)), date)).toBe(
      '398 assets deleted on 2026-10-15 by Priya Nair. 9 kept.',
    )
    expect(resultLine(t, removed([]), date)).toBe('398 assets deleted on 2026-10-15 by Priya Nair.')
  })

  it('leaves the name out for someone no longer in the workspace', () => {
    expect(resultLine(t, removed([], null), date)).toBe('398 assets deleted on 2026-10-15.')
  })
})

describe('previousValuesHelp', () => {
  it('names how many fields were blank before when there are any', () => {
    expect(previousValuesHelp(t, 3)).toBe(
      "Import this file to put the previous values back. A field that was blank before can't be restored this way, because blank cells never change a value; 3 such fields are listed in the import's What changes card.",
    )
    expect(previousValuesHelp(t, 0)).toBe(
      "Import this file to put the previous values back. A field that was blank before can't be restored this way, because blank cells never change a value.",
    )
  })
})
