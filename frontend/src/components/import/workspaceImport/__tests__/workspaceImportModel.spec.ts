import { describe, expect, it } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'

import enUS from '../../../../../../i18n/locales/en-US/main.ftl?raw'
import frFR from '../../../../../../i18n/locales/fr-FR/main.ftl?raw'
import nlNL from '../../../../../../i18n/locales/nl-NL/main.ftl?raw'

import {
  AREAS,
  filesLine,
  COUNT_KEYS,
  NOT_MOVING,
  archiveChecks,
  areaLines,
  attentionLines,
  checklist,
  countsLine,
  filterPeople,
  offLines,
  peopleGroups,
  personOutcome,
  refusalView,
  rowAction,
  stageIndex,
  statePill,
  stepFor,
  tiles,
  typedMatches,
  worthALook,
  type ArchiveContext,
} from '../workspaceImportModel'
import { PEOPLE, PLAN, job } from './fixtures'

const bundle = (source: string) => {
  const b = new FluentBundle('en-US', { useIsolating: false })
  b.addResource(new FluentResource(source))
  return b
}
const en = bundle(enUS)
const t = (key: string, args?: Record<string, string | number>) => {
  const message = en.getMessage(key)
  if (!message?.value) throw new Error(`missing ${key}`)
  return en.formatPattern(message.value, args)
}

const ctx = (over: Partial<ArchiveContext> = {}): ArchiveContext => ({
  job: null,
  file: null,
  uploading: false,
  refused: null,
  maxBytes: 1024 * 1024 * 1024,
  serverVersion: '1.1.3',
  workspace: 'Acme Helpdesk',
  ...over,
})

describe('stepFor', () => {
  it.each([
    ['uploaded', 'archive'],
    ['checking', 'archive'],
    ['previewed', 'preview'],
    ['importing', 'import'],
    ['imported', 'import'],
    ['failed', 'import'],
    ['reset', 'import'],
  ] as const)('puts a %s job on %s', (state, step) => {
    expect(stepFor(job(state))).toBe(step)
  })

  it('starts on Archive with no job', () => {
    expect(stepFor(null)).toBe('archive')
  })
})

describe('the Archive checks', () => {
  it('waits for a file, then reads the archive while it checks', () => {
    const idle = archiveChecks(ctx())
    expect(idle.map((l) => l.state)).toEqual(['todo', 'todo', 'todo'])
    const checking = archiveChecks(ctx({ job: job('checking') }))
    expect(checking.map((l) => l.state)).toEqual(['wait', 'ok', 'wait'])
  })

  it('passes all three once the job is previewed, naming the versions', () => {
    const lines = archiveChecks(ctx({ job: job('previewed') }))
    expect(lines.map((l) => l.state)).toEqual(['ok', 'ok', 'ok'])
    expect(lines[2]).toMatchObject({ title: 'workspace-import-check-version-ok', args: { export: '1.1.3', server: '1.1.3', slug: 'acme-it', origin: 'acme-it.nosdesk.com' } })
  })

  it('fails the row a refusal belongs to', () => {
    const refused = job('uploaded', { error: { code: 'workspace_not_empty', message: 'x', args: { found: [{ what: 'tickets', rows: 3 }, { what: 'files', rows: 4 }] } } })
    const lines = archiveChecks(ctx({ job: refused }))
    expect(lines[0].state).toBe('bad')
    expect(lines[0].refusal?.found).toEqual([{ what: 'tickets', rows: 3 }, { what: 'files', rows: 4 }])
    expect(lines[1].state).toBe('ok')
    // The archive was read before the workspace was checked.
    expect(lines[2].state).toBe('ok')

    const big = archiveChecks(ctx({ file: { name: 'big.nosdesk', size: 2 * 1024 ** 3 }, refused: { code: 'upload_too_large', message: '', args: { max: 1024 ** 3 } } }))
    expect(big[1]).toMatchObject({ state: 'bad', title: 'workspace-import-check-too-large', args: { size: 2 * 1024 ** 3, max: 1024 ** 3 } })
  })
})

describe('each refusal, its words and its fix', () => {
  it.each([
    ['workspace_not_empty', {}, 'workspace', 'workspace-import-check-not-empty', 'none'],
    ['import_in_progress', { job_id: 'other' }, 'workspace', 'workspace-import-check-in-progress', 'follow'],
    ['upload_too_large', { max: 10 }, 'size', 'workspace-import-check-too-large', 'choose_another'],
    ['workspace_import.format_too_old', { export_version: '1.1.2', server_version: '1.1.3' }, 'archive', 'workspace-import-check-format-too-old', 'choose_another'],
    ['workspace_import.earlier_version', { export_version: '1.1.2', server_version: '1.1.3' }, 'archive', 'workspace-import-check-version', 'choose_another'],
    ['workspace_import.newer_version', { export_version: '1.1.4', server_version: '1.1.3' }, 'archive', 'workspace-import-check-version', 'choose_another'],
    ['workspace_import.unknown_version', { export_version: '1.1.3-dev' }, 'archive', 'workspace-import-check-unknown-version', 'choose_another'],
    ['workspace_import.password_required', {}, 'archive', 'workspace-import-check-sealed', 'password'],
    ['workspace_import.password_wrong', {}, 'archive', 'workspace-import-check-password-wrong', 'password'],
    ['workspace_import.unreadable', {}, 'archive', 'workspace-import-check-unreadable', 'choose_another'],
    ['workspace_import.archive_missing', {}, 'archive', 'workspace-import-check-archive-missing', 'choose_another'],
    ['workspace_import.check_stopped', {}, 'archive', 'workspace-import-check-stopped', 'check_again'],
    ['import_failed', {}, 'archive', 'workspace-import-check-failed', 'check_again'],
    ['workspace_import.duplicate_email', { refusals: [{ code: 'workspace_import.duplicate_email', message: 'The export gives a@x to more than one person.' }] }, 'archive', 'workspace-import-check-plan', 'choose_another'],
    ['something_new', {}, 'archive', 'workspace-import-check-refused', 'choose_another'],
  ] as const)('%s', (code, args, row, title, fix) => {
    const view = refusalView({ code, message: 'English', args })
    expect(view).toMatchObject({ row, title, fix })
    // Every refusal reads in English with its arguments filled.
    const words = t(view.title, { workspace: 'Acme', server: '1.1.3', export: '1.1.2', size: '2 GB', max: '1 GB', ...view.args })
    expect(words).not.toMatch(/\{|\}/)
    expect(t(view.detail, { workspace: 'Acme', server: '1.1.3', export: '1.1.2', ...view.args })).not.toMatch(/\{|\}/)
  })

  it('words the versions from the refusal, the server version as a fallback', () => {
    const view = refusalView({ code: 'workspace_import.newer_version', message: '', args: { export_version: '1.1.4' } }, { serverVersion: '1.1.3' })
    expect(t(view.title, view.args)).toBe('Made by Nosdesk 1.1.4; this server runs 1.1.3')
    expect(t(view.detail, view.args)).toContain('Update this server to Nosdesk 1.1.4')
  })

  it('offers new ids for a uuid collision, and a fresh install for any other', () => {
    const uuid = refusalView({
      code: 'workspace_import.collision',
      message: '',
      args: { refusals: [{ code: 'workspace_import.collision', args: { table: 'documentation_pages', columns: ['uuid'], rows: 12 } }] },
    })
    expect(uuid).toMatchObject({ fix: 'new_ids', detail: 'workspace-import-check-collision-ids-detail', args: { rows: 12, table: 'documentation_pages' } })
    const slug = refusalView({
      code: 'workspace_import.collision',
      message: '',
      args: { refusals: [{ code: 'workspace_import.collision', args: { table: 'tags', columns: ['name'], rows: 2 } }] },
    })
    expect(slug.fix).toBe('none')
  })

  it('shows the plan refusals in the server English when there is no wording for them', () => {
    const view = refusalView({
      code: 'workspace_import.missing_parent',
      message: 'joined',
      args: { refusals: [{ message: 'First.' }, { message: 'Second.' }] },
    })
    expect(view.message).toBe('First. Second.')
  })
})

describe('the preview, from the report', () => {
  it('counts the tiles', () => {
    expect(tiles(PLAN)).toEqual({
      tickets: 2412,
      comments: 9874,
      pages: 180,
      collections: 6,
      assets: 612,
      projects: 9,
      people: 10,
      staff: 7,
      requesters: 3,
    })
  })

  it('lists what comes across, area by area, leaving out empty ones', () => {
    const lines = areaLines(PLAN)
    expect(lines.map((l) => l.id)).toEqual(['tickets', 'docs', 'assets', 'projects', 'people', 'config', 'integrations'])
    const ticketsLine = lines[0].counts.map((c) => t(c.key, { count: c.count }))
    expect(ticketsLine).toEqual(['2,412 tickets', '9,874 comments', '1,338 attachments', '38 merges'])
    expect(lines.find((l) => l.id === 'integrations')!.counts.map((c) => t(c.key, { count: c.count }))).toEqual([
      'outgoing mail settings',
      'a sign-in provider',
      '2 webhooks',
      '1 email channel',
      '2 plugins',
    ])
    expect(areaLines({ ...PLAN, tables: [] , people: []})).toEqual([])
  })

  it('groups the people and words where each one lands', () => {
    const groups = Object.fromEntries(peopleGroups(PEOPLE).map((g) => [g.key, g.count]))
    expect(groups).toEqual({
      'workspace-import-group-merged': 1,
      'workspace-import-group-staff-created': 4,
      'workspace-import-group-staff-mapped': 1,
      'workspace-import-group-staff-no-email': 1,
      'workspace-import-group-requesters-created': 2,
      'workspace-import-group-requesters-mapped': 1,
    })
    const by = (name: string) => PEOPLE.find((p) => p.name === name)!
    expect(personOutcome(by('Priya Natarajan')).key).toBe('workspace-import-outcome-merged')
    expect(personOutcome(by('Tom Okafor'))).toMatchObject({ key: 'workspace-import-outcome-created-was-owner', args: { role: 'admin' } })
    expect(personOutcome(by('Hannah Cole')).key).toBe('workspace-import-outcome-mapped')
    expect(personOutcome(by('Olivia Grant')).key).toBe('workspace-import-outcome-created-removed')
    expect(personOutcome(by('Ben Carter')).key).toBe('workspace-import-outcome-created-no-email')
    expect(personOutcome(by('Mei Lin'))).toMatchObject({ key: 'workspace-import-outcome-created', args: { role: 'admin' } })
  })

  it('filters staff, requesters and the people worth a look', () => {
    expect(filterPeople(PEOPLE, 'staff')).toHaveLength(7)
    expect(filterPeople(PEOPLE, 'requesters')).toHaveLength(3)
    expect(filterPeople(PEOPLE, 'attention').map((p) => p.name)).toEqual([
      'Tom Okafor',
      'Hannah Cole',
      'Olivia Grant',
      'Ben Carter',
      'Jonas Berg',
      'Ana Requester',
    ])
    expect(worthALook(PEOPLE[0])).toBe(false)
  })

  it('lists what arrives switched off, by integration', () => {
    expect(offLines(PLAN).map((l) => [l.id, l.count])).toEqual([
      ['workspace_email_settings', 1],
      ['workspace_identity_providers', 1],
      ['webhooks', 2],
      ['channels', 1],
      ['plugins', 2],
    ])
  })

  it('collects what needs attention', () => {
    const lines = attentionLines(PLAN).map((l) => t(l.key, l.args))
    expect(lines).toEqual([
      '1 rule names something the archive doesn\'t carry. It arrives as a draft and doesn\'t run until you fix it.',
      '1 saved view filters on something the archive doesn\'t carry. That filter is dropped; the rest of the view stays.',
      '4 mentions of people the archive doesn\'t carry become plain text.',
      '3 notifications rows depend on something the archive doesn\'t carry and are left out.',
      '1 person\'s username is already taken here. They arrive without one.',
    ])
  })

  it('works from the counts once the people are pruned', () => {
    const pruned = { ...PLAN, people: undefined }
    expect(tiles(pruned).people).toBe(10)
    expect(attentionLines(pruned).at(-1)).toMatchObject({ key: 'workspace-import-attention-usernames', args: { count: 1 } })
  })
})

describe('progress and Done', () => {
  it('follows the stages', () => {
    expect(stageIndex(job('importing', { stage: 'rows' }))).toBe(1)
    expect(stageIndex(job('importing', { stage: 'files' }))).toBe(2)
    expect(stageIndex(job('imported'))).toBe(4)
  })

  it('makes the checklist from what arrived off and what drafted', () => {
    const items = checklist(job('imported').report)
    expect(items.map((i) => i.id)).toEqual(['smtp', 'channels', 'sso', 'webhooks', 'plugins', 'rules', 'tokens'])
    expect(items.find((i) => i.id === 'webhooks')).toMatchObject({ args: { count: 2 }, to: { name: 'admin-webhooks' } })
    expect(checklist(null).map((i) => i.id)).toEqual(['tokens'])
  })

  it('counts the files restored, left out and still to restore', () => {
    expect(filesLine(job('imported'))).toEqual({ total: 1638, restored: 1636, skipped: 2, missing: 0 })
    const partial = job('failed')
    partial.report = { ...partial.report!, files: { restored: 1500, present: 100, skipped: [], failed: 'k' } }
    expect(filesLine(partial)).toEqual({ total: 1638, restored: 1600, skipped: 0, missing: 38 })
  })

  it('confirms only on the workspace name, ignoring case and spaces around it', () => {
    expect(typedMatches(' acme helpdesk ', 'Acme Helpdesk')).toBe(true)
    expect(typedMatches('Acme', 'Acme Helpdesk')).toBe(false)
    expect(typedMatches('', '')).toBe(false)
  })
})

describe('Recent imports', () => {
  it.each([
    ['uploaded', 'import-history-state-draft', 'resume'],
    ['previewed', 'import-history-state-draft', 'resume'],
    ['checking', 'workspace-import-state-checking', 'resume'],
    ['importing', 'workspace-import-state-importing', 'resume'],
    ['imported', 'import-history-state-imported', 'view'],
    ['reset', 'workspace-import-state-reset', 'view'],
  ] as const)('a %s job', (state, pill, action) => {
    expect(statePill(job(state)).key).toBe(pill)
    expect(rowAction(job(state))).toBe(action)
  })

  it('tells files to retry from a failure that imported nothing', () => {
    expect(statePill(job('failed')).key).toBe('workspace-import-state-files')
    expect(statePill(job('failed', { applied_at: null })).key).toBe('workspace-import-state-failed')
  })

  it('counts an import', () => {
    expect(countsLine(t, job('imported'))).toBe('2,412 tickets · 180 pages · 10 people')
    expect(countsLine(t, job('previewed'))).toBe('Draft on Preview')
  })
})

describe('the catalogues', () => {
  /** Every key the model can hand a component. */
  const keys = () => {
    const out = new Set<string>([...Object.values(COUNT_KEYS), ...NOT_MOVING.flatMap((n) => [n.label, n.detail])])
    for (const a of AREAS) {
      out.add(a.label)
      if (a.note) out.add(a.note)
    }
    for (const l of offLines(PLAN)) [l.label, l.detail, l.hint].forEach((k) => out.add(k))
    for (const l of attentionLines(PLAN)) out.add(l.key)
    for (const g of peopleGroups(PEOPLE)) out.add(g.key)
    for (const p of PEOPLE) out.add(personOutcome(p).key)
    for (const i of checklist(job('imported').report)) [i.title, i.detail, i.go].forEach((k) => out.add(k))
    return [...out]
  }

  it.each([
    ['en-US', enUS],
    ['fr-FR', frFR],
    ['nl-NL', nlNL],
  ])('%s has every key and no dashes', (_, source) => {
    const b = bundle(source)
    const missing = keys().filter((k) => !b.hasMessage(k))
    expect(missing).toEqual([])
    const block = source.slice(source.indexOf('workspace-import-title'))
    expect(block).not.toMatch(/[\u2013\u2014]|\x20-{2}\x20/)
  })
})
