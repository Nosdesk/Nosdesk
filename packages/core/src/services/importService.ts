import apiClient from '../apiClient'

export type ImportJobType = 'assets' | 'users' | 'tickets'

export type ImportJobStatus =
  | 'parsed'
  | 'dry_run_done'
  | 'committing'
  | 'done'
  | 'failed'

export interface RowError {
  row: number
  /** Field id, or null for a whole-row error. */
  column: string | null
  /** The file's header the field was mapped from. */
  source_column?: string | null
  message: string
}

/** Where a problem is fixed: an answer on Match values, a column or a
 *  constant on Map columns, or in the file (download the rows). */
export type ImportFix = 'match_values' | 'map' | 'download'

export interface ErrorGroup {
  column: string | null
  source_column?: string | null
  message: string
  count: number
  /** Absent on summaries checked before fixes were classified. */
  fix?: ImportFix
}

/** One field updated rows change. */
export interface ChangeCount {
  /** Field id. */
  field: string
  source_column?: string | null
  /** Rows that change it. */
  count: number
  /** A constant set for every row, existing records included, writes it. */
  constant?: boolean
}

/** How many records of one kind the import creates. */
export interface KindCount {
  /** Kind slug. */
  kind: string
  count: number
}

/** A column of the file no field takes. */
export interface UnmappedColumn {
  /** The source column, 0-based. */
  index: number
  /** Rows with a value in it. */
  values: number
}

/** Something about the plan that isn't an error. `message` is the
 *  English fallback for a code the client doesn't know. */
export interface ImportNotice {
  code: string
  count: number
  message: string
}

/** Values of one field rows import without: the rows' kind has no such
 *  attribute, (groups) nothing links the value, or (primary user) the
 *  person isn't a member. */
export interface DroppedGroup {
  /** Field id. */
  column: string
  source_column?: string | null
  /** The rows' kind (slug). */
  kind: string
  /** The value left out, for a group nothing links or a person who isn't
   *  a member; absent for an attribute. */
  value?: string
  /** Rows. */
  count: number
}

/** A model an import creates. */
export interface PlannedModel {
  manufacturer: string
  name: string
  /** Kind slug. */
  kind: string
}

/** Catalog entries an import creates, named as the file first spells them. */
export interface CatalogPlan {
  manufacturers: string[]
  models: PlannedModel[]
  groups: string[]
}

export interface ImportSummary {
  row_count: number
  would_create: number
  would_update: number
  /** Rows that match a record and would write nothing to it. */
  unchanged?: number
  notices?: ImportNotice[]
  errors: RowError[]
  errors_truncated: boolean
  errors_total?: number
  rows_with_errors?: number
  error_groups?: ErrorGroup[]
  /** Values left out: attributes the row's kind lacks, groups left off. */
  dropped?: DroppedGroup[]
  /** Catalog entries the import creates; absent when none. */
  catalog?: CatalogPlan
  /** Fields updated rows change, most first. */
  changes?: ChangeCount[]
  /** Kinds of the records the import creates, most first. Assets only. */
  kinds?: KindCount[]
  /** Columns no field takes (none chosen, or Don't import), in source
   *  order. */
  unmapped?: UnmappedColumn[]
}

export interface SourceHeader {
  index: number
  name: string
  samples: string[]
}

export interface ImportSheet {
  name: string
  /** False for a hidden sheet. */
  visible: boolean
  /** Data rows under the sheet's header row; one more than the cap
   *  (5,001) when it has more than an import takes. */
  rows: number
}

/** One row of the chosen sheet's top, blank rows included. */
export interface SheetPreviewRow {
  /** The sheet row number, 1-based. */
  row: number
  cells: string[]
}

export interface ImportSource {
  format: 'csv' | 'xlsx'
  sheets: ImportSheet[]
  sheet: string | null
  /** The header's line (CSV) or sheet row (xlsx), 1-based. */
  header_row: number
  headers: SourceHeader[]
  row_count: number
  delimiter?: string | null
  /** Workbooks only: the sheet's top, to confirm the header row. */
  preview?: SheetPreviewRow[]
}

export type SuggestionReason = 'previous' | 'exact' | 'label' | 'synonym' | 'report'

/** The import a saved column match came from. */
export interface SuggestionProvenance {
  job_id: string
  /** The admin who started that import. */
  by: string | null
  by_name: string | null
  at: string
}

export interface Suggestion {
  index: number
  /** null for a column that shouldn't be imported: an error report's own
   *  columns, or one the last import set to Don't import. */
  field: string | null
  /** `previous` is a saved match from the type's last mapped import. */
  reason: SuggestionReason
  provenance?: SuggestionProvenance
}

/** Field id to the 0-based column index that feeds it. */
export type ColumnMapping = Record<string, number>

export type MatchBy = 'asset_tag' | 'serial_number'
/** Which rows write: matched rows update and the others create, or only
 *  one of the two (the other is skipped). Assets only. */
export type ImportMode = 'create_update' | 'create_only' | 'update_only'
export type DateOrder = 'iso' | 'dmy' | 'mdy'
export type DecimalMark = '.' | ','

/** Which rows a constant writes to: rows that create a record, or those
 *  and rows that update one. */
export type ApplyTo = 'new' | 'all'

/** A value for one field on every row, typed like a cell. */
export interface ImportConstant {
  field: string
  value: string
  apply_to?: ApplyTo
}

export interface ImportOptions {
  match_by?: MatchBy
  mode?: ImportMode
  default_kind?: string | null
  date_order?: DateOrder
  decimal?: DecimalMark | null
  constants?: ImportConstant[]
}

/** What happens to the rows holding one source value of a field. */
export type ValueAnswer =
  | { action: 'match'; target: string | number }
  | { action: 'create' }
  | { action: 'keep_text' }
  | { action: 'leave_blank' }
  | { action: 'skip' }
  | { action: 'replace'; value: string }

/** Field id, then context (empty when the field has none), then the source
 *  value, to the admin's answer. */
export type ValueAnswers = Record<string, Record<string, Record<string, ValueAnswer>>>

export type FieldGroup = 'core' | 'links' | 'lifecycle' | 'attribute'
export type FieldValueType =
  | 'text'
  | 'enum'
  | 'number'
  | 'date'
  | 'date_time'
  | 'boolean'
  | 'array'
  | 'email'
  | 'user'
  | 'asset_ref'

export interface ImportField {
  id: string
  label: string
  label_key?: string
  group: FieldGroup
  value_type: FieldValueType
  required: boolean
  /** Kind slugs that define the field (attributes only). */
  kinds?: string[]
  enum?: string[]
  format?: string
  /** Kind slugs whose new assets need the field (attributes only). Never
   *  blocks the check; rows of those kinds that lack it are skipped. */
  required_for?: string[]
}

/** What Recent imports and the import detail call a job. Expired is a
 *  draft untouched for 30 days, or one whose file was removed: it can't be
 *  resumed. */
export type ImportJobState = 'draft' | 'imported' | 'expired'

export interface ImportJob {
  id: string
  job_type: ImportJobType
  status: ImportJobStatus
  /** Absent for a refused upload or a failed commit. */
  state?: ImportJobState | null
  /** When the 30-day prune removed the file and the per-row detail: the
   *  job is counts, options and mapping from then on. */
  pruned_at?: string | null
  /** Who uploaded it, as the workspace names them (the job's own read). */
  created_by_name?: string | null
  filename: string
  created_by: string | null
  created_at: string
  updated_at: string
  completed_at: string | null
  summary: ImportSummary | null
  records_committed: number | null
  error_message: string | null
  source: ImportSource | null
  suggested_mapping: Suggestion[]
  mapping: ColumnMapping | null
  /** Columns the stored mapping sets to Don't import. */
  ignored?: number[]
  options: ImportOptions | null
  answers?: ValueAnswers | null
  /** For a committed asset import, when Remove imported assets closes:
   *  30 days after the commit. */
  removal_closes_at?: string | null
  /** When Remove imported assets ran, by whom and what it did. */
  removed_at?: string | null
  removed_by?: string | null
  removed_by_name?: string | null
  remove_result?: RemoveResult | null
  /** The previous values file's size, while the details are kept (the
   *  job's own read, asset imports only). */
  previous_values?: PreviousValuesCounts | null
  /** For a committed workbook import whose file is kept, the next visible
   *  sheet with rows that nothing from this upload has imported (the job's
   *  own read and the commit's answer). */
  next_sheet?: string | null
  /** For a committed users import, the people it created who can't sign in
   *  yet and how they will (the job's own read and the commit's answer);
   *  absent when nobody is waiting. */
  sign_in?: SignInStatus | null
}

/** Done's sign-in line for a users import. */
export interface SignInStatus {
  /** People the import created who can't sign in yet. */
  pending: number
  /** Of those, the ones holding an invitation that is still open. */
  invited: number
  /** `invite`: an invitation sets their password. `sso`: they sign in with
   *  the workspace's provider once their accounts exist there. */
  method: 'invite' | 'sso'
  provider?: string | null
  /** For `invite`: whether email is set up, so invites can be sent. */
  email_ready: boolean
}

/** What Send invites did. */
export interface InvitesResult {
  sent: number
  failed: number
  sign_in: SignInStatus | null
}

/** Why an asset an import created is kept by Remove imported assets. The
 *  first of an asset's reasons is the one shown. */
export type KeptReason =
  | { code: 'synced'; source: string }
  | { code: 'disposed' }
  | { code: 'loan' }
  | { code: 'status_changed' }
  | { code: 'ticket'; ticket_id: number }
  | { code: 'media'; photos: number; files: number }
  | { code: 'audited'; at: string }
  | { code: 'usage' }
  | { code: 'directory' }
  | { code: 'referenced'; asset_id: number; name: string }
  | { code: 'group_added' }
  | { code: 'edited'; at: string; by?: string | null; by_name?: string | null }

export interface KeptAsset {
  id: number
  tag?: string | null
  name: string
  reasons: KeptReason[]
}

/** How many catalog entries the import created would be removed when
 *  nothing else uses them. */
export interface RemovalCatalogCounts {
  manufacturers: number
  models: number
  groups: number
}

/** What Remove imported assets would do now. */
export interface RemovalPlan {
  created: number
  deletable: number
  /** Deleted some other way since. */
  gone: number
  kept: KeptAsset[]
  catalog: RemovalCatalogCounts
  closes_at: string | null
}

/** What Remove imported assets did. */
export interface RemoveResult {
  created: number
  deleted: number
  gone: number
  deleted_ids: number[]
  kept: KeptAsset[]
}

export interface PreviousValuesCounts {
  /** Updated assets in the file. */
  rows: number
  /** Fields that were blank before, which the file can't put back. */
  blank_before: number
}

/** How a value matched: its entry's key or label (`exact`), the
 *  workspace's saved answer, a known other name or close spelling
 *  (`guessed`), or nothing. */
export type ValueLayer = 'exact' | 'saved' | 'guessed' | 'no_match'

export type ValueReason =
  | { code: 'synonym' }
  | { code: 'close_to'; label: string }
  | { code: 'disabled'; label: string; note: string }
  | { code: 'unreadable' }
  /** Names more than one catalog entry, told apart only by case. */
  | { code: 'ambiguous'; labels: string[] }
  /** Another value would be created too and is spelled close to this one. */
  | { code: 'both_new'; text: string }
  /** A model nothing matches, on rows with no manufacturer it could be
   *  created under. */
  | { code: 'needs_manufacturer' }

/** An entry a value can link to. */
export interface ValueChoice {
  value: string
  label: string
  /** Why it can't be picked here, when it can't. */
  disabled?: string
  /** More about the entry: a model's manufacturer, a member's email. */
  detail?: string
}

/** What a lookup column links to: a catalog, whose values can also be
 *  created and kept as text except a group's, or the workspace's members,
 *  who are linked or left unassigned. */
export type ValueLookup = 'manufacturer' | 'model' | 'group' | 'person'

/** One distinct value of a mapped column. */
export interface ImportValue {
  /** The value as answers key it: trimmed, lowercased, spaces collapsed;
   *  empty for blank cells. */
  key: string
  /** Its first spelling in the file. */
  text: string
  blank: boolean
  /** File lines holding it. */
  rows: number[]
  match: ValueLayer
  reason?: ValueReason
  /** What matching suggests; absent when the cell is kept as it is. */
  suggested?: ValueAnswer
  /** The admin's answer stored on the job. */
  answer?: ValueAnswer
  /** For a saved match, the import it came from. */
  saved?: SuggestionProvenance
  /** Rows holding it that the last check skipped. */
  skipped_rows: number[]
}

export interface ImportValueColumn {
  field: string
  /** The row context the column answers for; empty or absent when the
   *  field is answered the same on every row. Attributes are answered per
   *  kind (slug), models per the row's manufacturer (its name, lowercased;
   *  empty for rows without one). */
  context?: string
  /** The context's name. */
  context_label?: string
  /** Set for a column whose values link to catalog entries or members. */
  lookup?: ValueLookup
  /** For a lookup column, whether its values can be created: never a
   *  member, nor a model on rows with no manufacturer to belong to. */
  can_create?: boolean
  /** The source column. */
  index: number
  /** The file's header. */
  header: string
  value_type: FieldValueType
  required: boolean
  /** Entries a value can link to; empty for numbers and dates. */
  choices: ValueChoice[]
  /** Distinct values in the column. */
  distinct: number
  /** Of those, matched exactly. */
  matched: number
  /** Every value that isn't an exact match, then exact ones (at most 200). */
  values: ImportValue[]
}

/** What Match values reads: each mapped column whose values link to a
 *  closed set or must read as a number or date. */
export interface ImportValues {
  rows_checked: number
  rows_skipped: number
  /** Skipped rows no answer explains. */
  skipped_other: number[]
  columns: ImportValueColumn[]
}

export type RowOutcome = 'create' | 'update' | 'unchanged' | 'skipped'
export type RowFilter = 'all' | RowOutcome

/** One row's result: before the commit what it would do, after it what
 *  it did. */
export interface ImportRowResult {
  /** The row's line in the file; the header is line 1. */
  row: number
  outcome: RowOutcome
  /** The value the row is matched on (asset tag, serial, email). */
  key?: string
  /** What the row is called (asset name, person, ticket title). */
  label?: string
  /** The asset's kind (slug), for a row that checks out. */
  kind?: string
  /** The record the row matches (before the commit) or wrote (after). */
  record_id?: number | string
  errors?: { column: string | null; source_column?: string | null; message: string }[]
  /** Values the row imports without: its kind has no such attribute, a
   *  group left off, a person who isn't a member. */
  dropped?: { column: string; source_column?: string | null; kind: string; value?: string }[]
  /** Each field the row changes, before and after, as text (status and
   *  kind as their keys): after the commit what it wrote, in a checked plan
   *  what an update would write (a new row: its status move). */
  fields_written?: Record<string, { before: string | null; after: string | null }>
  groups_added?: number[]
  catalog_created?: { kind: 'manufacturer' | 'model' | 'group'; id: number; observed_updated_at?: string }[]
  /** The last lifecycle event the commit wrote: the status move, else the
   *  Imported event of a new asset. */
  transition_id?: number
  written_at?: string
  observed_updated_at?: string
}

export interface ImportRowsPage {
  phase: 'checked' | 'committed'
  total: number
  offset: number
  limit: number
  counts: Record<RowFilter, number>
  rows: ImportRowResult[]
}

/** What a saved match does in the workspace. */
export type SavedOutcome =
  | 'link'
  | 'kept_as_text'
  | 'left_as_is'
  | 'left_off'
  | 'left_unassigned'
  | 'left_blank'

/** Who saved a match, and in which import; `by` is null once the person's
 *  account is gone. */
export interface SavedBy {
  by: string | null
  by_name: string | null
  at: string
  job_id: string | null
}

/** One saved match: a value's spelling in a file and what it becomes. */
export interface SavedMatch {
  id: string
  field: string
  field_label: string
  label_key?: string
  /** Empty when the field is answered the same on every row. */
  context: string
  /** The context's name: a kind, a manufacturer. */
  context_label?: string
  text: string
  outcome: SavedOutcome
  /** For a link, the entry's id or key and its name. */
  target?: string
  label?: string
  /** The import that saved it created the entry. */
  created: boolean
  saved: SavedBy
}

/** One header of the latest mapped import and where it went. */
export interface SavedColumn {
  header: string
  field?: string
  field_label?: string
  label_key?: string
  /** Set to Don't import. */
  ignored: boolean
}

export interface SavedColumns {
  job_id: string
  by: string | null
  by_name: string | null
  at: string
  rows: SavedColumn[]
}

/** The Saved matches of one import type, in field order, and the header
 *  memory of its latest mapped import. */
export interface SavedMatches {
  job_type: ImportJobType
  entries: SavedMatch[]
  columns: SavedColumns | null
}

/** New, Updated, Unchanged and Skipped, as the tiles count them. */
export interface ImportCounts {
  new: number
  updated: number
  unchanged: number
  skipped: number
}

/** Every job type Recent imports and the import detail show: the
 *  spreadsheet types and documentation (`docsImportService`). */
export type AnyImportJobType = ImportJobType | 'documentation'

/** One job on Recent imports. */
export interface ImportJobListItem {
  id: string
  job_type: AnyImportJobType
  /** `checking` is a documentation import's background check. */
  status: ImportJobStatus | 'checking'
  state: ImportJobState | null
  filename: string
  /** The workbook sheet, for an .xlsx. */
  sheet: string | null
  created_by: string | null
  created_by_name: string | null
  created_at: string
  updated_at: string
  completed_at: string | null
  pruned_at: string | null
  /** Once the job has been checked. */
  counts: ImportCounts | null
  /** Once Remove imported assets ran: assets deleted of those created. */
  removed?: { deleted: number; created: number } | null
}

export const importJobsQueryKey = (limit: number) => ['import-jobs', limit] as const
export const importJobQueryKey = (jobId: string) => ['import-job', jobId] as const
export const importFieldsQueryKey = (jobType: ImportJobType) => ['import-fields', jobType] as const
export const importAnswersQueryKey = (jobType: ImportJobType) =>
  ['import-answers', jobType] as const
export const importValuesQueryKey = (jobId: string, updatedAt: string) =>
  ['import-values', jobId, updatedAt] as const
export const importValuesPreviewQueryKey = (jobId: string, updatedAt: string, answersKey: string) =>
  ['import-values-preview', jobId, updatedAt, answersKey] as const

/** Run a download; when it fails, read its body, a Blob, back as the JSON
 *  error the server sent, so the caller can read its `code`. */
export async function withJsonError<T>(run: () => Promise<T>): Promise<T> {
  try {
    return await run()
  } catch (e) {
    const response = (e as { response?: { data?: unknown } })?.response
    if (response && typeof Blob !== 'undefined' && response.data instanceof Blob) {
      try {
        response.data = JSON.parse(await response.data.text())
      } catch {
        // Not JSON: the caller words the failure itself.
      }
    }
    throw e
  }
}

export const importService = {
  /** Upload + parse. Returns the job in status `parsed` with the file's
   *  headers, samples and a suggested mapping. */
  async upload(jobType: ImportJobType, file: File): Promise<ImportJob> {
    const form = new FormData()
    form.append('file', file)
    const { data } = await apiClient.post<ImportJob>('/admin/import', form, {
      params: { type: jobType },
      headers: { 'Content-Type': 'multipart/form-data' },
    })
    return data
  },

  /** Read a workbook again from another sheet or header row (guessed
   *  when null). Returns the job back in `parsed` with the new columns
   *  and suggestions; any earlier mapping and check are cleared. */
  async putSource(jobId: string, sheet: string, headerRow: number | null): Promise<ImportJob> {
    const body: { sheet: string; header_row?: number } = { sheet }
    if (headerRow !== null) body.header_row = headerRow
    const { data } = await apiClient.put<ImportJob>(`/admin/import/${jobId}/source`, body)
    return data
  },

  /** Import the same file as another type, while nothing is committed.
   *  Returns the job back in `parsed` with suggestions for the new type;
   *  its mapping, options and check are cleared. */
  async putType(jobId: string, jobType: ImportJobType): Promise<ImportJob> {
    const { data } = await apiClient.put<ImportJob>(`/admin/import/${jobId}/type`, {
      type: jobType,
    })
    return data
  },

  /** Import the next sheet: a new draft on `sheet` of a committed
   *  import's workbook, with nothing carried but the file. Answers an open
   *  draft on that sheet from the same file instead of a second one. */
  async nextSheet(jobId: string, sheet: string): Promise<ImportJob> {
    const { data } = await apiClient.post<ImportJob>(`/admin/import/${jobId}/next-sheet`, {
      sheet,
    })
    return data
  },

  /** Send invites: an invitation to each person a committed users import
   *  created who can't sign in yet and holds no open invitation. */
  async sendInvites(jobId: string): Promise<InvitesResult> {
    const { data } = await apiClient.post<InvitesResult>(`/admin/import/${jobId}/invites`)
    return data
  },

  /** Discard a job nothing was imported from, with its files. A
   *  committed job answers 409. */
  async remove(jobId: string): Promise<void> {
    await apiClient.delete(`/admin/import/${jobId}`)
  },

  /** Store the mapping, the columns set to Don't import and the options,
   *  and run the dry run. Returns the job in status `dry_run_done` with its
   *  summary. */
  async putMapping(
    jobId: string,
    mapping: ColumnMapping,
    options: ImportOptions,
    ignored: number[] = [],
  ): Promise<ImportJob> {
    const { data } = await apiClient.put<ImportJob>(`/admin/import/${jobId}/mapping`, {
      mapping,
      ignored,
      options,
    })
    return data
  },

  /** The distinct values of each mapped column with how they matched and
   *  their answers. The job must be checked. */
  async values(jobId: string): Promise<ImportValues> {
    const { data } = await apiClient.get<ImportValues>(`/admin/import/${jobId}/values`)
    return data
  },

  /** The values as `answers` would match them, without storing them or
   *  checking the rows: Match values reads the Model card again for
   *  manufacturer answers not yet checked. */
  async previewValues(jobId: string, answers: ValueAnswers): Promise<ImportValues> {
    const { data } = await apiClient.post<ImportValues>(`/admin/import/${jobId}/values/preview`, {
      answers,
    })
    return data
  },

  /** Store the answers to the file's values, replacing any before, and
   *  check the rows again. Returns the job with the fresh summary. */
  async putValues(jobId: string, answers: ValueAnswers): Promise<ImportJob> {
    const { data } = await apiClient.put<ImportJob>(`/admin/import/${jobId}/values`, { answers })
    return data
  },

  /** Apply the checked job. Returns the final job row with
   *  `records_committed` populated. A 409 with code
   *  `import_plan_changed` carries the job with the fresh plan under
   *  `job`; `import_commit_running` means the commit is already running. */
  async commit(jobId: string): Promise<ImportJob> {
    const { data } = await apiClient.post<ImportJob>(`/admin/import/${jobId}/commit`)
    return data
  },

  /** Recent imports: the newest jobs that reached Map, newest first. */
  async list(limit = 20): Promise<ImportJobListItem[]> {
    const { data } = await apiClient.get<{ jobs: ImportJobListItem[] }>('/admin/import', {
      params: { limit },
    })
    return data.jobs
  },

  async get(jobId: string): Promise<ImportJob> {
    const { data } = await apiClient.get<ImportJob>(`/admin/import/${jobId}`)
    return data
  },

  /** The fields a column can map to, in picker order. */
  /** The type's saved matches and header memory. */
  async answers(jobType: ImportJobType): Promise<SavedMatches> {
    const { data } = await apiClient.get<SavedMatches>('/admin/import/answers', {
      params: { type: jobType },
    })
    return data
  },

  /** Delete one saved match. */
  async deleteAnswer(id: string): Promise<void> {
    await apiClient.delete(`/admin/import/answers/${id}`)
  },

  /** Delete every saved match of a type: how many went. */
  async deleteAnswers(jobType: ImportJobType): Promise<number> {
    const { data } = await apiClient.delete<{ deleted: number }>('/admin/import/answers', {
      params: { type: jobType },
    })
    return data.deleted
  },

  async fields(jobType: ImportJobType): Promise<ImportField[]> {
    const { data } = await apiClient.get<{ fields: ImportField[] }>('/admin/import/fields', {
      params: { type: jobType },
    })
    return data.fields
  },

  /** One page of the job's row results, filtered by outcome. */
  async rows(
    jobId: string,
    params: { filter?: RowFilter; offset?: number; limit?: number } = {},
  ): Promise<ImportRowsPage> {
    const { data } = await apiClient.get<ImportRowsPage>(`/admin/import/${jobId}/rows`, {
      params,
    })
    return data
  },

  /** The skipped rows as CSV, in the file's layout with the reasons.
   *  `group` narrows it to one of the summary's error groups (by index);
   *  `dropped` lists instead the rows that import without some of their
   *  values, for those dropped groups (by index). */
  async errorReport(
    jobId: string,
    only: { group?: number; dropped?: number[] } = {},
  ): Promise<{ blob: Blob; filename: string }> {
    const params: Record<string, string | number> = {}
    if (only.group !== undefined) params.group = only.group
    if (only.dropped?.length) params.dropped = only.dropped.join(',')
    const response = await withJsonError(() =>
      apiClient.get<Blob>(`/admin/import/${jobId}/error-report`, {
        params,
        responseType: 'blob',
      }),
    )
    const disposition = String(response.headers['content-disposition'] ?? '')
    const match = /filename="([^"]+)"/.exec(disposition)
    return { blob: response.data, filename: match?.[1] ?? 'import-errors.csv' }
  },

  /** What Remove imported assets would do now: the dialog's counts and
   *  kept list. */
  async removalPlan(jobId: string): Promise<RemovalPlan> {
    const { data } = await apiClient.get<RemovalPlan>(`/admin/import/${jobId}/remove-created`)
    return data
  },

  /** Remove imported assets: the job with what it did. */
  async removeCreated(jobId: string): Promise<ImportJob> {
    const { data } = await apiClient.post<ImportJob>(`/admin/import/${jobId}/remove-created`)
    return data
  },

  /** The previous values file: one row per updated asset with the values
   *  the import replaced. */
  async previousValues(jobId: string): Promise<{ blob: Blob; filename: string }> {
    const response = await withJsonError(() =>
      apiClient.get<Blob>(`/admin/import/${jobId}/previous-values`, {
        responseType: 'blob',
      }),
    )
    const disposition = String(response.headers['content-disposition'] ?? '')
    const match = /filename="([^"]+)"/.exec(disposition)
    return { blob: response.data, filename: match?.[1] ?? 'previous-values.csv' }
  },

  /** Build a URL for the CSV template; trigger via anchor
   *  download in the caller so the browser handles the file
   *  save rather than us decoding the response body. For assets, `kind`
   *  (a slug) adds that kind's attribute columns. */
  templateUrl(jobType: ImportJobType, kind?: string): string {
    const base = `/api/admin/import/template/${jobType}`
    return kind ? `${base}?kind=${encodeURIComponent(kind)}` : base
  },
}
