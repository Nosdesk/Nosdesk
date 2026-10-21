/**
 * The workspace import's rules, kept out of the components so they can be
 * tested on their own: which step a job is on, the Archive step's three
 * checks and each refusal's fix, the preview read from the plan's report,
 * the people list, the progress stages, Done's checklist, and the row
 * Recent imports shows.
 *
 * Everything reads the job as the server answers it
 * (`services/workspace_import_jobs::view`); nothing here is invented. Where
 * the report doesn't carry something the design shows (the archive
 * workspace's name, integration names), the wording falls back to what it
 * does carry (the slug, counts).
 */
import type {
  WorkspaceImportJob,
  WorkspaceImportPerson,
  WorkspaceImportPlan,
  WorkspaceImportRefusal,
  WorkspaceImportReport,
  WorkspaceImportState,
} from '@nosdesk/core/services/workspaceImportService'

import type { StatusPillTone } from '@/components/common/statusPillTone'

export type Translate = (key: string, args?: Record<string, string | number>) => string

/** The upload cap when the server hasn't said otherwise: 1 GiB, the
 *  export's own cap (`NOSDESK_MAX_IMPORT_BYTES` on the server). */
export const DEFAULT_MAX_BYTES = 1024 * 1024 * 1024

/** How long a job keeps its report and its reset (the server's KEEP_FOR). */
export const KEEP_DAYS = 30

export type WizardStep = 'archive' | 'preview' | 'import'
export const STEPS: WizardStep[] = ['archive', 'preview', 'import']

/** The step a job is on. */
export function stepFor(job: Pick<WorkspaceImportJob, 'state'> | null): WizardStep {
  switch (job?.state) {
    case 'previewed':
      return 'preview'
    case 'importing':
    case 'imported':
    case 'failed':
    case 'reset':
      return 'import'
    default:
      return 'archive'
  }
}

/** States a background run is in; the page follows the job until it
 *  leaves them. */
export const RUNNING: WorkspaceImportState[] = ['checking', 'importing']
export const isRunning = (job: Pick<WorkspaceImportJob, 'state'> | null) =>
  !!job && RUNNING.includes(job.state)

/** A job that still holds the workspace: one at a time. */
export const isLive = (job: Pick<WorkspaceImportJob, 'state'> | null) =>
  !!job && ['uploaded', 'checking', 'previewed', 'importing'].includes(job.state)

/** The rows committed but some files didn't restore. */
export const filesPartial = (job: Pick<WorkspaceImportJob, 'state' | 'applied_at'>) =>
  job.state === 'failed' && !!job.applied_at

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/** Which check a refusal fails. */
export type CheckRow = 'workspace' | 'size' | 'archive'

const WORKSPACE_CODES = new Set(['workspace_not_empty', 'import_in_progress', 'workspace_not_found'])
const SIZE_CODES = new Set(['upload_too_large'])

export function rowOf(code: string): CheckRow {
  if (WORKSPACE_CODES.has(code)) return 'workspace'
  if (SIZE_CODES.has(code)) return 'size'
  return 'archive'
}

/** The plan refusal codes, without their prefix. */
const PLAN_CODES = new Set([
  'not_workspace_tables',
  'collision',
  'unknown_person',
  'missing_parent',
  'reference_cycle',
  'duplicate_person',
  'duplicate_email',
  'bad_row',
  'refused',
])

/** What fixes a refusal, as the button the row offers. */
export type Fix =
  | 'none'
  | 'choose_another'
  | 'password'
  | 'check_again'
  | 'new_ids'
  | 'follow'

export interface RefusalView {
  row: CheckRow
  /** Fluent key of the failed row's title. */
  title: string
  /** Fluent key of its explanation. */
  detail: string
  args: Record<string, string | number>
  fix: Fix
  /** What the workspace already holds (`workspace_not_empty`). */
  found?: { what: string; rows: number }[]
  /** The server's English, for a refusal this page has no wording for. */
  message?: string
}

const str = (v: unknown, fallback = ''): string =>
  typeof v === 'string' ? v : typeof v === 'number' ? String(v) : fallback
const num = (v: unknown): number => (typeof v === 'number' && Number.isFinite(v) ? v : 0)

/** A refusal's row, words and fix. `serverVersion` fills a version the
 *  refusal didn't carry. */
export function refusalView(
  refusal: WorkspaceImportRefusal,
  context: { serverVersion?: string; maxBytes?: number } = {},
): RefusalView {
  const args = refusal.args ?? {}
  const code = refusal.code
  const versions = {
    export: str(args.export_version, '?'),
    server: str(args.server_version, context.serverVersion ?? '?'),
  }
  const base = { row: rowOf(code), args: {} as Record<string, string | number> }
  switch (code) {
    case 'workspace_not_empty': {
      const found = Array.isArray(args.found)
        ? (args.found as { what?: unknown; rows?: unknown }[]).map((f) => ({ what: str(f.what), rows: num(f.rows) }))
        : []
      return { ...base, title: 'workspace-import-check-not-empty', detail: 'workspace-import-check-not-empty-detail', fix: 'none', found }
    }
    case 'import_in_progress':
      return {
        ...base,
        title: 'workspace-import-check-in-progress',
        detail: 'workspace-import-check-in-progress-detail',
        args: { job: str(args.job_id) },
        fix: str(args.job_id) ? 'follow' : 'none',
      }
    case 'workspace_not_found':
      return { ...base, title: 'workspace-import-check-no-workspace', detail: 'workspace-import-check-no-workspace-detail', fix: 'none' }
    case 'upload_too_large':
      return {
        ...base,
        title: 'workspace-import-check-too-large',
        detail: 'workspace-import-check-too-large-detail',
        args: { max: num(args.max) || context.maxBytes || DEFAULT_MAX_BYTES },
        fix: 'choose_another',
      }
    case 'workspace_import.format_too_old':
      return { ...base, title: 'workspace-import-check-format-too-old', detail: 'workspace-import-check-format-too-old-detail', args: versions, fix: 'choose_another' }
    case 'workspace_import.earlier_version':
      return { ...base, title: 'workspace-import-check-version', detail: 'workspace-import-check-earlier-detail', args: versions, fix: 'choose_another' }
    case 'workspace_import.newer_version':
      return { ...base, title: 'workspace-import-check-version', detail: 'workspace-import-check-newer-detail', args: versions, fix: 'choose_another' }
    case 'workspace_import.unknown_version':
      return { ...base, title: 'workspace-import-check-unknown-version', detail: 'workspace-import-check-unknown-version-detail', args: versions, fix: 'choose_another' }
    case 'workspace_import.password_required':
      return { ...base, title: 'workspace-import-check-sealed', detail: 'workspace-import-check-sealed-detail', fix: 'password' }
    case 'workspace_import.password_wrong':
      return { ...base, title: 'workspace-import-check-password-wrong', detail: 'workspace-import-check-password-wrong-detail', fix: 'password' }
    case 'workspace_import.unreadable':
      return { ...base, title: 'workspace-import-check-unreadable', detail: 'workspace-import-check-unreadable-detail', fix: 'choose_another' }
    case 'workspace_import.archive_missing':
      return { ...base, title: 'workspace-import-check-archive-missing', detail: 'workspace-import-check-archive-missing-detail', fix: 'choose_another' }
    case 'workspace_import.check_stopped':
      return { ...base, title: 'workspace-import-check-stopped', detail: 'workspace-import-check-stopped-detail', fix: 'check_again' }
    case 'workspace_import.import_stopped':
      return { ...base, title: 'workspace-import-check-import-stopped', detail: 'workspace-import-check-import-stopped-detail', fix: 'none' }
    case 'import_failed':
      return { ...base, title: 'workspace-import-check-failed', detail: 'workspace-import-check-failed-detail', fix: 'check_again' }
    default:
      break
  }
  const plan = code.startsWith('workspace_import.') ? code.slice('workspace_import.'.length) : ''
  if (PLAN_CODES.has(plan)) {
    const first = firstPlanRefusal(args)
    if (plan === 'collision') {
      const columns = Array.isArray(first?.args?.columns) ? (first!.args!.columns as unknown[]).map((c) => str(c)) : []
      return {
        ...base,
        title: 'workspace-import-check-collision',
        detail: columns.includes('uuid') ? 'workspace-import-check-collision-ids-detail' : 'workspace-import-check-collision-detail',
        args: { rows: num(first?.args?.rows), table: str(first?.args?.table) },
        fix: columns.includes('uuid') ? 'new_ids' : 'none',
      }
    }
    return {
      ...base,
      title: 'workspace-import-check-plan',
      detail: 'workspace-import-check-plan-detail',
      fix: 'choose_another',
      message: planMessages(args) || refusal.message,
    }
  }
  return { ...base, title: 'workspace-import-check-refused', detail: 'workspace-import-check-refused-detail', fix: 'choose_another', message: refusal.message }
}

function firstPlanRefusal(args: Record<string, unknown>): { code?: string; args?: Record<string, unknown> } | null {
  const list = Array.isArray(args.refusals) ? (args.refusals as { code?: string; args?: Record<string, unknown> }[]) : []
  return list[0] ?? null
}

function planMessages(args: Record<string, unknown>): string {
  const list = Array.isArray(args.refusals) ? (args.refusals as { message?: unknown }[]) : []
  return list.map((r) => str(r.message)).filter(Boolean).join(' ')
}

// ---------------------------------------------------------------------------
// The Archive step's checks
// ---------------------------------------------------------------------------

export type CheckState = 'ok' | 'bad' | 'wait' | 'todo'

export interface CheckLine {
  row: CheckRow
  state: CheckState
  title: string
  detail?: string
  args: Record<string, string | number>
  refusal?: RefusalView
}

export interface ArchiveContext {
  job: WorkspaceImportJob | null
  /** The file chosen or uploading, before there is a job. */
  file: { name: string; size: number } | null
  uploading: boolean
  /** A refusal that came back on a request, before any job held it. */
  refused: WorkspaceImportRefusal | null
  maxBytes: number
  serverVersion: string
  /** This workspace's name. */
  workspace: string
}

/** The three checks: this workspace is untouched, the file fits, the
 *  archive is one this server imports. Each fills in as it runs; a refusal
 *  fails its own row. */
export function archiveChecks(ctx: ArchiveContext): CheckLine[] {
  const refusal = ctx.refused ?? (ctx.job?.state === 'uploaded' ? ctx.job.error : null)
  const view = refusal ? refusalView(refusal, { serverVersion: ctx.serverVersion, maxBytes: ctx.maxBytes }) : null
  const failed = (row: CheckRow): CheckLine | null =>
    view && view.row === row
      ? {
          row,
          state: 'bad',
          title: view.title,
          detail: view.detail,
          args: { workspace: ctx.workspace, server: ctx.serverVersion, size: ctx.job?.archive.size ?? ctx.file?.size ?? 0, ...view.args },
          refusal: view,
        }
      : null

  const job = ctx.job
  const checking = job?.state === 'checking'
  const previewed = !!job && job.state !== 'uploaded' && job.state !== 'checking'
  const size = job?.archive.size ?? ctx.file?.size ?? 0

  const workspace: CheckLine =
    failed('workspace') ??
    (previewed
      ? { row: 'workspace', state: 'ok', title: 'workspace-import-check-untouched', detail: 'workspace-import-check-untouched-detail', args: { workspace: ctx.workspace } }
      : {
          row: 'workspace',
          state: checking ? 'wait' : 'todo',
          title: 'workspace-import-check-untouched-pending',
          detail: 'workspace-import-check-untouched-pending-detail',
          args: { workspace: ctx.workspace },
        })

  const sizeLine: CheckLine =
    failed('size') ??
    (size > 0
      ? { row: 'size', state: 'ok', title: 'workspace-import-check-size-ok', args: { size, max: ctx.maxBytes } }
      : { row: 'size', state: 'todo', title: 'workspace-import-check-size-pending', args: { max: ctx.maxBytes } })

  const source = job?.report?.source
  const archive: CheckLine =
    failed('archive') ??
    (previewed && source
      ? {
          row: 'archive',
          state: 'ok',
          title: 'workspace-import-check-version-ok',
          detail: 'workspace-import-check-version-ok-detail',
          args: {
            export: source.nosdesk_version,
            server: ctx.serverVersion || source.nosdesk_version,
            slug: source.workspace_slug,
            date: source.created_at,
            origin: hostOf(source.origin) || '-',
          },
        }
      : job && view?.row === 'workspace'
        ? // The archive was read before the workspace was checked.
          { row: 'archive', state: 'ok', title: 'workspace-import-check-version-pending', args: { server: ctx.serverVersion } }
        : checking
        ? { row: 'archive', state: 'wait', title: 'workspace-import-check-reading', detail: 'workspace-import-check-reading-detail', args: {} }
        : { row: 'archive', state: 'todo', title: 'workspace-import-check-version-pending', detail: 'workspace-import-check-version-pending-detail', args: { server: ctx.serverVersion } })

  return [workspace, sizeLine, archive]
}

/** The host of an origin URL, or the value as it is. */
export function hostOf(origin: string | null | undefined): string {
  if (!origin) return ''
  try {
    return new URL(origin).host
  } catch {
    return origin
  }
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

/** A table's planned rows, 0 when the plan doesn't name it. */
export function planned(plan: WorkspaceImportPlan | undefined, table: string): number {
  return plan?.tables.find((t) => t.table === table)?.planned ?? 0
}

/** A count phrase per table: "2,412 tickets". Each key takes `count`. */
export const COUNT_KEYS: Record<string, string> = {
  tickets: 'workspace-import-n-tickets',
  comments: 'workspace-import-n-comments',
  attachments: 'workspace-import-n-attachments',
  ticket_merges: 'workspace-import-n-merges',
  documentation_pages: 'workspace-import-n-pages',
  documentation_collections: 'workspace-import-n-collections',
  documentation_revisions: 'workspace-import-n-revisions',
  assets: 'workspace-import-n-assets',
  asset_loans: 'workspace-import-n-loans',
  asset_disposals: 'workspace-import-n-disposals',
  asset_kinds: 'workspace-import-n-asset-kinds',
  asset_media: 'workspace-import-n-asset-photos',
  projects: 'workspace-import-n-projects',
  cycles: 'workspace-import-n-cycles',
  groups: 'workspace-import-n-groups',
  workflow_states: 'workspace-import-n-workflow-states',
  ticket_categories: 'workspace-import-n-categories',
  sla_policies: 'workspace-import-n-sla',
  working_calendars: 'workspace-import-n-calendars',
  saved_views: 'workspace-import-n-views',
  rules: 'workspace-import-n-rules',
  assignment_rules: 'workspace-import-n-assignment-rules',
  canned_responses: 'workspace-import-n-replies',
  webhooks: 'workspace-import-n-webhooks',
  channels: 'workspace-import-n-channels',
  plugins: 'workspace-import-n-plugins',
}

/** The areas of What comes across, each with the tables it counts. */
export const AREAS: { id: string; label: string; tables: string[]; note?: string }[] = [
  { id: 'tickets', label: 'workspace-import-area-tickets', tables: ['tickets', 'comments', 'attachments', 'ticket_merges'], note: 'workspace-import-area-tickets-note' },
  { id: 'docs', label: 'workspace-import-area-docs', tables: ['documentation_pages', 'documentation_collections', 'documentation_revisions'], note: 'workspace-import-area-docs-note' },
  { id: 'assets', label: 'workspace-import-area-assets', tables: ['assets', 'asset_loans', 'asset_disposals', 'asset_kinds', 'asset_media'] },
  { id: 'projects', label: 'workspace-import-area-projects', tables: ['projects', 'cycles'] },
  { id: 'people', label: 'workspace-import-area-people', tables: ['groups'], note: 'workspace-import-area-people-note' },
  {
    id: 'config',
    label: 'workspace-import-area-config',
    tables: ['workflow_states', 'ticket_categories', 'sla_policies', 'working_calendars', 'saved_views', 'rules', 'assignment_rules', 'canned_responses'],
  },
  { id: 'integrations', label: 'workspace-import-area-integrations', tables: ['webhooks', 'channels', 'plugins'], note: 'workspace-import-area-integrations-note' },
]

export interface AreaLine {
  id: string
  label: string
  /** Each table's count phrase, in order, nonzero only. */
  counts: { key: string; count: number }[]
  note?: string
}

/** What comes across, area by area. An area with nothing in it is left out,
 *  except People, which counts the people themselves. */
export function areaLines(plan: WorkspaceImportPlan | undefined): AreaLine[] {
  const out: AreaLine[] = []
  for (const area of AREAS) {
    const counts = area.tables
      .map((table) => ({ key: COUNT_KEYS[table], count: planned(plan, table) }))
      .filter((c) => c.key && c.count > 0)
    if (area.id === 'people') {
      const people = peopleSplit(plan)
      const lead = [
        { key: 'workspace-import-n-staff', count: people.staff },
        { key: 'workspace-import-n-requesters', count: people.requesters },
      ].filter((c) => c.count > 0)
      if (lead.length + counts.length === 0) continue
      out.push({ id: area.id, label: area.label, counts: [...lead, ...counts], note: area.note })
      continue
    }
    if (area.id === 'integrations') {
      const settings = integrationSettings(plan)
      if (settings.length + counts.length === 0) continue
      out.push({ id: area.id, label: area.label, counts: [...settings.map((key) => ({ key, count: 1 })), ...counts], note: area.note })
      continue
    }
    if (counts.length === 0) continue
    out.push({ id: area.id, label: area.label, counts, note: area.note })
  }
  return out
}

/** Single-row integration settings the archive carries. */
const SETTINGS_TABLES: Record<string, string> = {
  workspace_email_settings: 'workspace-import-integration-smtp',
  workspace_identity_providers: 'workspace-import-integration-sso',
  workspace_ldap_settings: 'workspace-import-integration-ldap',
  workspace_widget_settings: 'workspace-import-integration-widget',
}

function integrationSettings(plan: WorkspaceImportPlan | undefined): string[] {
  return Object.entries(SETTINGS_TABLES)
    .filter(([table]) => planned(plan, table) > 0)
    .map(([, key]) => key)
}

export interface Tiles {
  tickets: number
  comments: number
  pages: number
  collections: number
  assets: number
  projects: number
  people: number
  staff: number
  requesters: number
}

export function tiles(plan: WorkspaceImportPlan | undefined): Tiles {
  const people = peopleSplit(plan)
  return {
    tickets: planned(plan, 'tickets'),
    comments: planned(plan, 'comments'),
    pages: planned(plan, 'documentation_pages'),
    collections: planned(plan, 'documentation_collections'),
    assets: planned(plan, 'assets'),
    projects: planned(plan, 'projects'),
    people: people.total,
    staff: people.staff,
    requesters: people.requesters,
  }
}

/** Staff and requesters in the archive. Once the report's people are
 *  pruned, the counts are all there is. */
export function peopleSplit(plan: WorkspaceImportPlan | undefined): { total: number; staff: number; requesters: number } {
  const people = plan?.people
  if (!people) {
    const c = plan?.counts
    const total = c ? c.merged_into_runner + c.mapped + c.created + c.placeholders : 0
    return { total, staff: 0, requesters: 0 }
  }
  const staff = people.filter((p) => p.staff).length
  return { total: people.length, staff, requesters: people.length - staff }
}

// ---------------------------------------------------------------------------
// People
// ---------------------------------------------------------------------------

export interface PeopleGroup {
  key: string
  count: number
}

/** How the people come across, as the groups at the top of People. */
export function peopleGroups(people: WorkspaceImportPerson[]): PeopleGroup[] {
  const n = (f: (p: WorkspaceImportPerson) => boolean) => people.filter(f).length
  const groups: PeopleGroup[] = [
    { key: 'workspace-import-group-merged', count: n((p) => p.outcome === 'merged_into_runner') },
    { key: 'workspace-import-group-staff-created', count: n((p) => p.staff && p.outcome === 'created' && !!p.email) },
    { key: 'workspace-import-group-staff-mapped', count: n((p) => p.staff && p.outcome === 'mapped') },
    { key: 'workspace-import-group-staff-no-email', count: n((p) => p.staff && p.outcome === 'created' && !p.email) },
    { key: 'workspace-import-group-requesters-created', count: n((p) => !p.staff && p.outcome === 'created') },
    { key: 'workspace-import-group-requesters-mapped', count: n((p) => !p.staff && p.outcome === 'mapped') },
    { key: 'workspace-import-group-placeholders', count: n((p) => p.outcome === 'placeholder') },
  ]
  return groups.filter((g) => g.count > 0)
}

export type OutcomeTone = 'you' | 'warn' | 'faint' | 'plain'

/** Where one person lands, in words. */
export function personOutcome(p: WorkspaceImportPerson): { key: string; args: Record<string, string>; tone: OutcomeTone } {
  if (p.outcome === 'merged_into_runner') return { key: 'workspace-import-outcome-merged', args: {}, tone: 'you' }
  if (p.outcome === 'placeholder') return { key: 'workspace-import-outcome-placeholder', args: {}, tone: 'faint' }
  if (p.outcome === 'mapped') return { key: 'workspace-import-outcome-mapped', args: {}, tone: 'warn' }
  if (!p.role_on_target) {
    return { key: p.disabled ? 'workspace-import-outcome-created-removed' : 'workspace-import-outcome-created-no-member', args: {}, tone: 'faint' }
  }
  if (!p.email) return { key: 'workspace-import-outcome-created-no-email', args: { role: p.role_on_target }, tone: 'warn' }
  const wasOwner = p.archive_role === 'owner' && p.role_on_target !== 'owner'
  return {
    key: wasOwner ? 'workspace-import-outcome-created-was-owner' : 'workspace-import-outcome-created',
    args: { role: p.role_on_target },
    tone: 'plain',
  }
}

/** A person worth checking before the import: someone who maps onto an
 *  account already here, a former owner, someone removed or without an
 *  email, or whose username was taken. */
export function worthALook(p: WorkspaceImportPerson): boolean {
  if (p.outcome === 'merged_into_runner') return false
  return (
    p.disabled ||
    p.outcome === 'mapped' ||
    p.outcome === 'placeholder' ||
    !p.email ||
    p.username_dropped ||
    (p.archive_role === 'owner' && p.role_on_target !== 'owner')
  )
}

export type PeopleFilter = 'staff' | 'attention' | 'requesters'

export function filterPeople(people: WorkspaceImportPerson[], filter: PeopleFilter): WorkspaceImportPerson[] {
  switch (filter) {
    case 'staff':
      return people.filter((p) => p.staff || p.outcome === 'merged_into_runner')
    case 'requesters':
      return people.filter((p) => !p.staff && p.outcome !== 'merged_into_runner')
    case 'attention':
      return people.filter(worthALook)
  }
}

// ---------------------------------------------------------------------------
// Arrives switched off, doesn't move, needs attention
// ---------------------------------------------------------------------------

export interface OffLine {
  id: string
  label: string
  detail: string
  hint: string
  count: number
}

/** Integrations by the table they live in: label, what happened, the fix. */
const OFF_TABLES: Record<string, { label: string; detail: string; hint: string }> = {
  workspace_email_settings: { label: 'workspace-import-off-smtp', detail: 'workspace-import-off-smtp-detail', hint: 'workspace-import-off-smtp-hint' },
  workspace_identity_providers: { label: 'workspace-import-off-sso', detail: 'workspace-import-off-sso-detail', hint: 'workspace-import-off-sso-hint' },
  workspace_ldap_settings: { label: 'workspace-import-off-ldap', detail: 'workspace-import-off-ldap-detail', hint: 'workspace-import-off-ldap-hint' },
  webhooks: { label: 'workspace-import-off-webhooks', detail: 'workspace-import-off-webhooks-detail', hint: 'workspace-import-off-webhooks-hint' },
  channels: { label: 'workspace-import-off-channels', detail: 'workspace-import-off-channels-detail', hint: 'workspace-import-off-channels-hint' },
  plugins: { label: 'workspace-import-off-plugins', detail: 'workspace-import-off-plugins-detail', hint: 'workspace-import-off-plugins-hint' },
  workspace_widget_settings: { label: 'workspace-import-off-widget', detail: 'workspace-import-off-widget-detail', hint: 'workspace-import-off-widget-hint' },
}

/** What arrives switched off, from the plan's handover: one line per
 *  integration table, the most rows any of its switches turned off. */
export function offLines(plan: WorkspaceImportPlan | undefined): OffLine[] {
  const byTable = new Map<string, number>()
  for (const h of plan?.handover ?? []) {
    if (h.kind === 'switched_off' || h.kind === 'encrypted_setting_dropped') {
      byTable.set(h.table, Math.max(byTable.get(h.table) ?? 0, h.rows))
    } else if (h.kind === 'plugin_secrets_dropped') {
      byTable.set('plugins', Math.max(byTable.get('plugins') ?? 0, planned(plan, 'plugins')))
    }
  }
  const out: OffLine[] = []
  for (const [table, words] of Object.entries(OFF_TABLES)) {
    const count = byTable.get(table)
    if (!count) continue
    out.push({ id: table, ...words, count: table === 'webhooks' || table === 'channels' || table === 'plugins' ? Math.max(count, planned(plan, table)) : count })
  }
  return out
}

/** Tokens the import gives new values (guest links to tickets). */
export function regeneratedTokens(plan: WorkspaceImportPlan | undefined): number {
  return (plan?.handover ?? [])
    .filter((h) => h.kind === 'tokens_regenerated')
    .reduce((n, h) => n + h.rows, 0)
}

/** What never moves. Fixed: the policy never carries these. */
export const NOT_MOVING: { label: string; detail: string }[] = [
  { label: 'workspace-import-not-tokens', detail: 'workspace-import-not-tokens-detail' },
  { label: 'workspace-import-not-devices', detail: 'workspace-import-not-devices-detail' },
  { label: 'workspace-import-not-activity', detail: 'workspace-import-not-activity-detail' },
  { label: 'workspace-import-not-passwords', detail: 'workspace-import-not-passwords-detail' },
  { label: 'workspace-import-not-history', detail: 'workspace-import-not-history-detail' },
]

export interface AttentionLine {
  key: string
  args: Record<string, string | number>
}

const EFFECT_KEYS: Record<string, string> = {
  rule_drafted: 'workspace-import-attention-rules',
  predicate_dropped: 'workspace-import-attention-views',
  cleared: 'workspace-import-attention-cleared',
  row_dropped: 'workspace-import-attention-rows',
  mention_as_text: 'workspace-import-attention-mentions',
  ticket_link_as_text: 'workspace-import-attention-ticket-links',
  image_as_text: 'workspace-import-attention-images',
  embed_as_text: 'workspace-import-attention-embeds',
  not_a_document: 'workspace-import-attention-documents',
}

/** What the importer couldn't keep, one line per effect (and table, for a
 *  cleared value or a dropped row), then the usernames taken here. */
export function attentionLines(plan: WorkspaceImportPlan | undefined): AttentionLine[] {
  if (!plan) return []
  const totals = new Map<string, AttentionLine>()
  const add = (key: string, table: string, count: number) => {
    const id = `${key}:${table}`
    const line = totals.get(id) ?? { key, args: { count: 0, table: tableWord(table) } }
    line.args.count = Number(line.args.count) + count
    totals.set(id, line)
  }
  for (const r of plan.references ?? []) {
    const key = EFFECT_KEYS[r.effect]
    if (!key) continue
    const perTable = r.effect === 'cleared' || r.effect === 'row_dropped'
    add(key, perTable ? r.table : '', r.count)
  }
  for (const t of plan.tables ?? []) {
    for (const [, rows] of Object.entries(t.dropped ?? {})) add('workspace-import-attention-rows', t.table, rows)
    for (const [, rows] of Object.entries(t.cleared ?? {})) add('workspace-import-attention-cleared', t.table, rows)
  }
  const out = [...totals.values()]
  const usernames = plan.people ? plan.people.filter((p) => p.username_dropped).length : (plan.counts?.usernames_dropped ?? 0)
  if (usernames > 0) out.push({ key: 'workspace-import-attention-usernames', args: { count: usernames } })
  return out
}

/** A table's name as words: `asset_loans` -> "asset loans". */
export function tableWord(table: string): string {
  return table.replace(/_/g, ' ')
}

// ---------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------

export type StageId = 'check' | 'rows' | 'files' | 'finish'
export const STAGES: StageId[] = ['check', 'rows', 'files', 'finish']

/** How far a running import is: the index of the stage under way. */
export function stageIndex(job: Pick<WorkspaceImportJob, 'state' | 'stage'>): number {
  if (job.state === 'imported') return STAGES.length
  if (job.state !== 'importing') return 0
  if (job.stage === 'files') return 2
  return 1
}

// ---------------------------------------------------------------------------
// Done
// ---------------------------------------------------------------------------

export interface ChecklistItem {
  id: string
  title: string
  detail: string
  args: Record<string, string | number>
  /** Where it is fixed. */
  to: { name: string }
  go: string
}

/** The to-do list Done shows: each integration that arrived off, each
 *  rule that arrived as a draft, new API tokens, and the name. */
export function checklist(report: WorkspaceImportReport | null): ChecklistItem[] {
  const plan = report?.plan
  const items: ChecklistItem[] = []
  const off = new Set(offLines(plan).map((l) => l.id))
  const count = (table: string) => offLines(plan).find((l) => l.id === table)?.count ?? 0
  if (off.has('workspace_email_settings')) {
    items.push({ id: 'smtp', title: 'workspace-import-todo-smtp', detail: 'workspace-import-todo-smtp-detail', args: {}, to: { name: 'admin-email-delivery' }, go: 'workspace-import-todo-go-email' })
  }
  if (off.has('channels')) {
    items.push({ id: 'channels', title: 'workspace-import-todo-channels', detail: 'workspace-import-todo-channels-detail', args: { count: count('channels') }, to: { name: 'admin-channels' }, go: 'workspace-import-todo-go-channels' })
  }
  if (off.has('workspace_identity_providers')) {
    items.push({ id: 'sso', title: 'workspace-import-todo-sso', detail: 'workspace-import-todo-sso-detail', args: {}, to: { name: 'admin-auth-providers' }, go: 'workspace-import-todo-go-sso' })
  }
  if (off.has('workspace_ldap_settings')) {
    items.push({ id: 'ldap', title: 'workspace-import-todo-ldap', detail: 'workspace-import-todo-ldap-detail', args: {}, to: { name: 'admin-ldap' }, go: 'workspace-import-todo-go-ldap' })
  }
  if (off.has('webhooks')) {
    items.push({ id: 'webhooks', title: 'workspace-import-todo-webhooks', detail: 'workspace-import-todo-webhooks-detail', args: { count: count('webhooks') }, to: { name: 'admin-webhooks' }, go: 'workspace-import-todo-go-webhooks' })
  }
  if (off.has('plugins')) {
    items.push({ id: 'plugins', title: 'workspace-import-todo-plugins', detail: 'workspace-import-todo-plugins-detail', args: { count: count('plugins') }, to: { name: 'admin-plugins' }, go: 'workspace-import-todo-go-plugins' })
  }
  if (off.has('workspace_widget_settings')) {
    items.push({ id: 'widget', title: 'workspace-import-todo-widget', detail: 'workspace-import-todo-widget-detail', args: {}, to: { name: 'admin-widget' }, go: 'workspace-import-todo-go-widget' })
  }
  const drafted = (plan?.references ?? []).filter((r) => r.effect === 'rule_drafted').reduce((n, r) => n + r.count, 0)
  if (drafted > 0) {
    items.push({ id: 'rules', title: 'workspace-import-todo-rules', detail: 'workspace-import-todo-rules-detail', args: { count: drafted }, to: { name: 'admin-rules' }, go: 'workspace-import-todo-go-rules' })
  }
  items.push({ id: 'tokens', title: 'workspace-import-todo-tokens', detail: 'workspace-import-todo-tokens-detail', args: {}, to: { name: 'admin-api-tokens' }, go: 'workspace-import-todo-go-tokens' })
  return items
}

/** Files the archive carried, restored, and still to restore. */
export function filesLine(job: WorkspaceImportJob): { total: number; restored: number; missing: number; skipped: number } {
  const total = job.report?.source?.files ?? 0
  const files = job.report?.files
  const restored = files ? files.restored + (files.present ?? 0) : job.files_restored
  const skipped = Array.isArray(files?.skipped) ? files.skipped.length : 0
  return { total, restored, skipped, missing: Math.max(0, total - restored - skipped) }
}

/** The day the reset stops being offered: 30 days after the job was last
 *  changed, as the prune counts it. */
export function resetUntil(job: Pick<WorkspaceImportJob, 'finished_at' | 'applied_at' | 'updated_at'>): string {
  const from = job.finished_at ?? job.applied_at ?? job.updated_at
  const at = new Date(from)
  if (Number.isNaN(at.getTime())) return ''
  at.setUTCDate(at.getUTCDate() + KEEP_DAYS)
  return at.toISOString()
}

/** Whether a typed confirmation names the workspace. */
export function typedMatches(typed: string, name: string): boolean {
  const a = typed.trim().toLocaleLowerCase()
  return a.length > 0 && a === name.trim().toLocaleLowerCase()
}

// ---------------------------------------------------------------------------
// Recent imports
// ---------------------------------------------------------------------------

export function statePill(job: Pick<WorkspaceImportJob, 'state' | 'applied_at'>): { key: string; tone: StatusPillTone } {
  switch (job.state) {
    case 'uploaded':
    case 'previewed':
      return { key: 'import-history-state-draft', tone: 'caution' }
    case 'checking':
      return { key: 'workspace-import-state-checking', tone: 'info' }
    case 'importing':
      return { key: 'workspace-import-state-importing', tone: 'info' }
    case 'imported':
      return { key: 'import-history-state-imported', tone: 'positive' }
    case 'failed':
      return job.applied_at
        ? { key: 'workspace-import-state-files', tone: 'caution' }
        : { key: 'workspace-import-state-failed', tone: 'critical' }
    case 'reset':
      return { key: 'workspace-import-state-reset', tone: 'neutral' }
  }
}

/** The row's action: a job still on the way resumes in the wizard, any
 *  other opens its page. */
export function rowAction(job: Pick<WorkspaceImportJob, 'state'>): 'resume' | 'view' {
  return isLive(job) ? 'resume' : 'view'
}

/** The counts line for Recent imports. */
export function countsLine(t: Translate, job: WorkspaceImportJob): string {
  if (job.state === 'reset') return t('workspace-import-recent-reset')
  if (job.state === 'uploaded' || job.state === 'checking') return t('workspace-import-recent-on-archive')
  if (job.state === 'previewed') return t('workspace-import-recent-on-preview')
  if (job.state === 'importing') return t('workspace-import-recent-importing')
  if (job.state === 'failed' && !job.applied_at) return t('workspace-import-recent-failed')
  const tl = tiles(job.report?.plan)
  return t('workspace-import-recent-counts', { tickets: tl.tickets, pages: tl.pages, people: tl.people })
}

/** The archive's name as the page shows it: the workspace it came from. */
export function archiveName(job: Pick<WorkspaceImportJob, 'report' | 'archive'>): string {
  return job.report?.source?.workspace_slug || job.archive.name || ''
}

export function wizardTarget(id: string) {
  return { name: 'admin-workspace-import', query: { job: id } }
}

export function detailTarget(id: string) {
  return { name: 'admin-workspace-import-detail', params: { id } }
}
