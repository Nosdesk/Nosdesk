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

export interface ImportSummary {
  row_count: number
  would_create: number
  would_update: number
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

export interface ImportSource {
  format: 'csv' | 'xlsx'
  sheets: { name: string; visible: boolean; rows: number }[]
  sheet: string | null
  header_row: number
  headers: SourceHeader[]
  row_count: number
  delimiter?: string | null
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

export interface ImportOptions {
  match_by?: MatchBy
  create_missing?: boolean
  default_kind?: string | null
  date_order?: DateOrder
  decimal?: DecimalMark | null
}

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
   *  `records_committed` populated. */
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

  /** Build a URL for the CSV template; trigger via anchor
   *  download in the caller so the browser handles the file
   *  save rather than us decoding the response body. */
  templateUrl(jobType: ImportJobType): string {
    return `/api/admin/import/template/${jobType}`
  },
}
