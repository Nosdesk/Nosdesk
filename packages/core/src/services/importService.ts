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
  column: string | null
  message: string
}

export interface ImportSummary {
  row_count: number
  would_create: number
  would_update: number
  errors: RowError[]
  errors_truncated: boolean
}

export interface ImportJob {
  id: string
  job_type: ImportJobType
  status: ImportJobStatus
  filename: string
  file_path: string
  created_by: string | null
  created_at: string
  updated_at: string
  completed_at: string | null
  summary: ImportSummary | null
  records_committed: number | null
  error_message: string | null
}

export const importService = {
  /** Upload + parse + dry-run. Returns the job with summary
   *  populated. The same job id is used for the subsequent
   *  commit. */
  async upload(jobType: ImportJobType, file: File): Promise<ImportJob> {
    const form = new FormData()
    form.append('file', file)
    const { data } = await apiClient.post<ImportJob>('/admin/import', form, {
      params: { type: jobType },
      headers: { 'Content-Type': 'multipart/form-data' },
    })
    return data
  },

  /** Apply the previously dry-run'd job. Returns the final
   *  job row with `records_committed` populated. */
  async commit(jobId: string): Promise<ImportJob> {
    const { data } = await apiClient.post<ImportJob>(`/admin/import/${jobId}/commit`)
    return data
  },

  async get(jobId: string): Promise<ImportJob> {
    const { data } = await apiClient.get<ImportJob>(`/admin/import/${jobId}`)
    return data
  },

  /** Build a URL for the CSV template; trigger via anchor
   *  download in the caller so the browser handles the file
   *  save rather than us decoding the response body. */
  templateUrl(jobType: ImportJobType): string {
    return `/api/admin/import/template/${jobType}`
  },
}
