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

export interface ErrorGroup {
  column: string | null
  source_column?: string | null
  message: string
  count: number
}

/** Something about the plan that isn't an error. `message` is the
 *  English fallback for a code the client doesn't know. */
export interface ImportNotice {
  code: string
  count: number
  message: string
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

export interface Suggestion {
  index: number
  /** null for a column that shouldn't be imported (an error report's own columns). */
  field: string | null
  reason: SuggestionReason
}

/** Field id to the 0-based column index that feeds it. */
export type ColumnMapping = Record<string, number>

export type MatchBy = 'asset_tag' | 'serial_number' | 'none'
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
  create_missing?: boolean
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
  kinds?: string[]
  enum?: string[]
  format?: string
}

export interface ImportJob {
  id: string
  job_type: ImportJobType
  status: ImportJobStatus
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
  options: ImportOptions | null
  answers?: ValueAnswers | null
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
  /** The record the row matches (before the commit) or wrote (after). */
  record_id?: number | string
  errors?: { column: string | null; source_column?: string | null; message: string }[]
  /** After the commit: each field the row changed, before and after. */
  fields_written?: Record<string, { before: string | null; after: string | null }>
  groups_added?: number[]
  catalog_created?: { kind: 'manufacturer' | 'model' | 'group'; id: number; observed_updated_at?: string }[]
  /** The lifecycle event the commit wrote (an Imported event). */
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

export const importFieldsQueryKey = (jobType: ImportJobType) => ['import-fields', jobType] as const

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

  /** Store the mapping and options and run the dry run. Returns the job
   *  in status `dry_run_done` with its summary. */
  async putMapping(
    jobId: string,
    mapping: ColumnMapping,
    options: ImportOptions,
  ): Promise<ImportJob> {
    const { data } = await apiClient.put<ImportJob>(`/admin/import/${jobId}/mapping`, {
      mapping,
      options,
    })
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

  async get(jobId: string): Promise<ImportJob> {
    const { data } = await apiClient.get<ImportJob>(`/admin/import/${jobId}`)
    return data
  },

  /** The fields a column can map to, in picker order. */
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

  /** The skipped rows as CSV, in the file's layout with the reasons. */
  async errorReport(jobId: string): Promise<{ blob: Blob; filename: string }> {
    const response = await apiClient.get<Blob>(`/admin/import/${jobId}/error-report`, {
      responseType: 'blob',
    })
    const disposition = String(response.headers['content-disposition'] ?? '')
    const match = /filename="([^"]+)"/.exec(disposition)
    return { blob: response.data, filename: match?.[1] ?? 'import-errors.csv' }
  },

  /** Build a URL for the CSV template; trigger via anchor
   *  download in the caller so the browser handles the file
   *  save rather than us decoding the response body. */
  templateUrl(jobType: ImportJobType): string {
    return `/api/admin/import/template/${jobType}`
  },
}
