/**
 * Documentation imports: a zip of Markdown (or loose `.md` files) becomes
 * linked pages, through the importer's jobs. The routes sit beside the
 * spreadsheet importer's (`importService`) and share its job list, detail
 * read and history; every route is workspace-admin only.
 *
 *  - `upload` answers with the job in `parsed`, its `source.tree` the pages
 *    as Nosdesk will show them under the default options.
 *  - `putOptions` with `check: false` reads the tree again (a changed
 *    profile or destination) and answers 200 in `parsed`; with `check: true`
 *    it starts the check in the background and answers 202 in `checking`.
 *    The job then lands in `dry_run_done`, or back in `parsed` with
 *    `error_message`.
 *  - `commit` answers 202 in `committing` (`records_committed` counts the
 *    pages written) or 409 `import_plan_changed` with the job's new plan
 *    under `job`.
 *
 * `get` follows a job through both background runs.
 */
import apiClient from '../apiClient'
import { withJsonError, type ImportJobState } from './importService'

export const DOCS_JOB_TYPE = 'documentation' as const

export type DocsJobStatus = 'parsed' | 'checking' | 'dry_run_done' | 'committing' | 'done' | 'failed'

/** Which tool wrote the archive. */
export type DocsProfile =
  | 'nosdesk_v2'
  | 'nosdesk_v1'
  | 'notion'
  | 'obsidian'
  | 'gitbook'
  | 'mkdocs'
  | 'docusaurus'
  | 'outline'
  | 'plain'

export const DOCS_PROFILES: DocsProfile[] = [
  'notion',
  'nosdesk_v2',
  'nosdesk_v1',
  'obsidian',
  'gitbook',
  'mkdocs',
  'docusaurus',
  'outline',
  'plain',
]

/** Where the pages go. */
export type DocsCollectionMode = 'new' | 'per_top_level_folder' | 'existing' | 'as_source'
export type DocsStatusMode = 'drafts' | 'published' | 'as_files'
export type DocsWebImages = 'download' | 'links'
export type DocsDuplicates = 'skip' | 'copy'

/** What the admin chose at Pages and Review. Unset fields take the
 *  server's defaults (the profile's collection mode, drafts, download). */
export interface DocsOptions {
  profile?: DocsProfile | null
  collections?: DocsCollectionMode | null
  collection_name?: string | null
  collection_id?: number | null
  under_page_id?: number | null
  status?: DocsStatusMode
  web_images?: DocsWebImages
  /** Archive paths left out, as globs. */
  leave_out?: string[]
  duplicates?: DocsDuplicates
}

/** A page of the tree as Pages shows it. */
export interface DocsTreePage {
  index: number
  title: string
  /** The Markdown file, or the folder a section page stands for. */
  path: string
  file: string | null
  folder: string | null
  parent: number | null
  order: number
  /** The new collection a top-level page goes in, by index. */
  collection: number | null
}

export interface DocsTreeView {
  profile: DocsProfile
  /** What detection said, when a chosen profile replaced it. */
  detected: DocsProfile | null
  /** The collections the import would create, by name. */
  collections: string[]
  pages: DocsTreePage[]
  section_pages: number
  images: number
  web_images: number
  left_out: number
}

export interface DocsSource {
  kind: 'documentation'
  size: number
  files: number
  tree: DocsTreeView
}

/** One kind of change the conversion made (`NoteCode` on the server). */
export type DocsNoteCode =
  | 'tables_hoisted'
  | 'table_rows_padded'
  | 'table_cells_flattened'
  | 'checklists_as_bullets'
  | 'strikethrough_removed'
  | 'callouts_as_quotes'
  | 'formulas_as_code'
  | 'links_dropped'
  | 'images_dropped'
  | 'html_removed'
  | 'html_as_text'
  | 'nesting_flattened'
  | 'table_cells_dropped'
  | 'mentions_as_text'
  | 'ticket_links_as_text'
  | 'embeds_as_text'
  | 'links_unresolved'
  | 'links_ambiguous'
  | 'attachments_left_out'
  | 'front_matter_ignored'
  | 'title_shortened'
  | 'not_utf8'
  | 'source_syntax_removed'
  | 'database_rows_cut'
  | 'images_unreadable'
  | 'svg_images_refused'
  | 'web_images_not_fetched'

export interface DocsNoteLine {
  code: DocsNoteCode
  count: number
  pages: number
}

export interface DocsDestination {
  mode: DocsCollectionMode
  new_collections: string[]
  collection_id: number | null
  collection_name: string | null
  under_page_id: number | null
}

/** A checked job's summary. Carries the spreadsheet importer's count keys,
 *  which Recent imports reads. */
export interface DocsSummary {
  kind: 'documentation'
  row_count: number
  would_create: number
  would_update: number
  unchanged: number
  rows_with_errors: number
  skipped: number
  /** Pages found in the workspace already, skipped or not. */
  duplicates: number
  /** Pages whose file couldn't be read, imported empty. */
  errors: number
  images: number
  image_problems: number
  web_images: number
  links_rewritten: number
  notes: DocsNoteLine[]
  section_pages: number
  left_out: number
  front_matter_not_imported: Record<string, number>
  profile: DocsProfile
  detected: DocsProfile | null
  destination: DocsDestination
  status: DocsStatusMode
  duplicates_choice: DocsDuplicates
  web_images_choice: DocsWebImages
}

export type DocsPageOutcome = 'new' | 'skipped' | 'error'
export type DocsImageKind = 'archive' | 'data' | 'web' | 'workspace'
export type DocsImageProblem =
  | 'unreadable'
  | 'too_large'
  | 'not_an_image'
  | 'svg'
  | 'refused_address'
  | 'timed_out'
  | 'fetch_failed'
  | 'over_limit'

export interface DocsPageImage {
  source: string
  kind: DocsImageKind
  name: string
  size: number | null
  problem?: DocsImageProblem
}

export interface DocsLinkReport {
  rewritten?: number
  unresolved?: string[]
  ambiguous?: Record<string, number[]>
  attachments?: string[]
  missing_images?: string[]
  images?: string[]
}

/** One page's result at the check. */
export interface DocsPageRow {
  index: number
  path: string
  title: string
  parent: number | null
  outcome: DocsPageOutcome
  duplicate?: { page_id: number; page_uuid: string; by: 'uuid' | 'source' }
  source_uuid?: string
  notes: Partial<Record<DocsNoteCode, number>>
  links: DocsLinkReport
  images?: DocsPageImage[]
  error?: string
}

export type DocsPagesFilter = 'all' | 'notes' | 'skipped' | 'errors'

export interface DocsPagesPage {
  total: number
  offset: number
  limit: number
  counts: Record<DocsPagesFilter, number>
  pages: DocsPageRow[]
}

export interface DocsPreviewImage {
  name: string
  size: number | null
  kind: DocsImageKind
  problem?: DocsImageProblem
}

/** One page as it will be written: ProseMirror JSON, with its images as
 *  named placeholders (`nosdesk-import:preview-image/{n}`) and links to
 *  pages in the import as `nosdesk-import:page/{index}`. */
export interface DocsPreview {
  index: number
  title: string
  doc: Record<string, unknown>
  images: Record<string, DocsPreviewImage>
}

export interface DocsImportJob {
  id: string
  job_type: 'documentation'
  status: DocsJobStatus
  state?: ImportJobState | null
  pruned_at?: string | null
  created_by_name?: string | null
  filename: string
  created_by: string | null
  created_at: string
  updated_at: string
  completed_at: string | null
  /** A checked job's summary; `{kind, run}` while a check runs. */
  summary: DocsSummary | { kind: 'documentation'; run: string } | null
  /** While committing, the pages written so far (updated about every 30
   *  seconds); once done, every page written. */
  records_committed: number | null
  error_message: string | null
  source: DocsSource | null
  options: DocsOptions | null
}

/** The checked summary, when the job has one. */
export function docsSummary(job: Pick<DocsImportJob, 'summary'> | null | undefined): DocsSummary | null {
  const s = job?.summary
  return s && 'row_count' in s ? s : null
}

export const docsPagesQueryKey = (jobId: string, updatedAt: string) =>
  ['docs-import-pages', jobId, updatedAt] as const
export const docsPreviewQueryKey = (jobId: string, updatedAt: string, index: number) =>
  ['docs-import-preview', jobId, updatedAt, index] as const

/** The largest page of results the server sends. */
const PAGE_LIMIT = 200

export const docsImportService = {
  /** Upload one `.zip`, or one or more `.md` files. */
  async upload(
    files: File[],
    onProgress?: (loaded: number, total: number) => void,
  ): Promise<DocsImportJob> {
    const form = new FormData()
    for (const file of files) form.append('file', file)
    const { data } = await apiClient.post<DocsImportJob>('/admin/import', form, {
      params: { type: DOCS_JOB_TYPE },
      headers: { 'Content-Type': 'multipart/form-data' },
      onUploadProgress: onProgress
        ? (e) => onProgress(e.loaded, e.total ?? files.reduce((n, f) => n + f.size, 0))
        : undefined,
    })
    return data
  },

  async get(jobId: string): Promise<DocsImportJob> {
    const { data } = await apiClient.get<DocsImportJob>(`/admin/import/${jobId}`)
    return data
  },

  /** Store the options: read the tree again (`check: false`) or start the
   *  check (`check: true`, the job answers in `checking`). */
  async putOptions(jobId: string, options: DocsOptions, check: boolean): Promise<DocsImportJob> {
    const { data } = await apiClient.put<DocsImportJob>(`/admin/import/${jobId}/docs/options`, {
      options,
      check,
    })
    return data
  },

  async pages(
    jobId: string,
    params: { filter?: DocsPagesFilter; offset?: number; limit?: number } = {},
  ): Promise<DocsPagesPage> {
    const { data } = await apiClient.get<DocsPagesPage>(`/admin/import/${jobId}/docs/pages`, {
      params,
    })
    return data
  },

  /** Every page's result, read in pages of 200. */
  async allPages(jobId: string): Promise<DocsPageRow[]> {
    const rows: DocsPageRow[] = []
    for (let offset = 0; ; offset += PAGE_LIMIT) {
      const page = await this.pages(jobId, { filter: 'all', offset, limit: PAGE_LIMIT })
      rows.push(...page.pages)
      if (page.pages.length === 0 || rows.length >= page.total) return rows
    }
  },

  async preview(jobId: string, index: number): Promise<DocsPreview> {
    const { data } = await apiClient.get<DocsPreview>(
      `/admin/import/${jobId}/docs/pages/${index}/preview`,
    )
    return data
  },

  /** The conversion report: every page with its outcome and notes. */
  async report(jobId: string): Promise<{ blob: Blob; filename: string }> {
    const response = await withJsonError(() =>
      apiClient.get<Blob>(`/admin/import/${jobId}/docs/report`, { responseType: 'blob' }),
    )
    const disposition = String(response.headers['content-disposition'] ?? '')
    const match = /filename="([^"]+)"/.exec(disposition)
    return { blob: response.data, filename: match?.[1] ?? 'conversion-report.csv' }
  },

  /** Start the commit with the duplicates choice made on Review. */
  async commit(jobId: string, duplicates?: DocsDuplicates): Promise<DocsImportJob> {
    const { data } = await apiClient.post<DocsImportJob>(
      `/admin/import/${jobId}/docs/commit`,
      duplicates ? { duplicates } : {},
    )
    return data
  },
}
