/**
 * Workspace imports: an export from another Nosdesk, uploaded into this
 * workspace while it has nothing in it, checked, previewed, confirmed and,
 * when unwanted, reset. Owner only, self-hosted only: on Nosdesk Cloud every
 * route answers 404, because a Cloud import runs from the account dashboard.
 *
 * A job moves `uploaded` -> `checking` -> `previewed` -> `importing` ->
 * `imported` (or `failed` with files still to restore) -> `reset`. A check
 * that refuses puts the job back in `uploaded` with `error`; a sealed archive
 * waits in `uploaded` for its password. `actions` says which buttons the job
 * offers now.
 */
import apiClient from '../apiClient'

export type WorkspaceImportState =
  | 'uploaded'
  | 'checking'
  | 'previewed'
  | 'importing'
  | 'imported'
  | 'failed'
  | 'reset'

/** Where a running import is: loading the rows, then restoring files. */
export type WorkspaceImportStage = 'rows' | 'files'

/** Why the server refused, as the job stores it. */
export interface WorkspaceImportRefusal {
  code: string
  message: string
  args?: Record<string, unknown>
}

/** The archive's manifest, as the check read it. */
export interface WorkspaceImportSource {
  nosdesk_version: string
  format: number
  created_at: string
  /** `self_hosted` or `hosted`: the kind of server that made it. */
  mode: string
  workspace_slug: string
  origin: string | null
  custom_domain: string | null
  sealed: boolean
  secrets: boolean
  /** Files the archive carries. */
  files: number
}

/** One archive table in the plan. */
export interface WorkspaceImportTable {
  table: string
  /** `move`, `replace`, `merge`, `never` or `excluded`. */
  class: string
  in_archive: number
  planned: number
  dropped?: Record<string, number>
  cleared?: Record<string, number>
}

export type WorkspaceImportOutcome = 'merged_into_runner' | 'mapped' | 'created' | 'placeholder'

/** One archive person and where they land. */
export interface WorkspaceImportPerson {
  archive_uuid: string
  target_uuid: string
  name: string
  email: string | null
  archive_role: string | null
  staff: boolean
  /** Removed from the workspace or deleted in the archive. */
  disabled: boolean
  outcome: WorkspaceImportOutcome
  placeholder?: 'staff' | 'no_email' | 'requester_unlinked'
  /** Their role here after the import; null: no membership. */
  role_on_target: string | null
  username_dropped: boolean
  uuid_reminted: boolean
}

export interface WorkspaceImportPeopleCounts {
  merged_into_runner: number
  mapped: number
  created: number
  placeholders: number
  requesters_unlinked: number
  usernames_dropped: number
  uuids_reminted: number
}

/** Something the import leaves for someone to redo or check. */
export type WorkspaceImportHandover =
  | { kind: 'encrypted_setting_dropped'; table: string; column: string; rows: number }
  | { kind: 'tokens_regenerated'; table: string; column: string; rows: number }
  | { kind: 'plugin_secrets_dropped'; rows: number }
  | { kind: 'switched_off'; table: string; column: string; rows: number }

export type WorkspaceImportEffect =
  | 'cleared'
  | 'rule_drafted'
  | 'predicate_dropped'
  | 'row_dropped'
  | 'mention_as_text'
  | 'ticket_link_as_text'
  | 'image_as_text'
  | 'embed_as_text'
  | 'not_a_document'

/** A reference the import couldn't keep, and what it did instead. */
export interface WorkspaceImportReference {
  table: string
  column: string
  effect: WorkspaceImportEffect
  count: number
}

/** The plan: what the import does, for the preview. */
export interface WorkspaceImportPlan {
  mode: string
  tables: WorkspaceImportTable[]
  /** Absent once the 30-day prune has dropped the people. */
  people?: WorkspaceImportPerson[]
  counts: WorkspaceImportPeopleCounts
  handover: WorkspaceImportHandover[]
  references: WorkspaceImportReference[]
  refusals: unknown[]
}

export interface WorkspaceImportFound {
  what: string
  rows: number
}

/** A file the restore left out, and why. */
export interface WorkspaceImportSkippedFile {
  path: string
  reason: 'unsafe_path' | 'refused' | 'not_imported' | 'missing'
  detail?: string
}

/** What the file restore did. */
export interface WorkspaceImportFiles {
  /** Files put. */
  restored: number
  /** Files already in place from an earlier run. */
  present: number
  skipped: WorkspaceImportSkippedFile[]
  /** The key whose put failed; the restore stopped there. */
  failed?: string
}

export interface WorkspaceImportReport {
  source?: WorkspaceImportSource
  plan?: WorkspaceImportPlan
  /** The seeded rows the import replaced. */
  cleared?: WorkspaceImportFound[]
  tables_imported?: number
  rows_imported?: number
  data_epoch?: number
  files?: WorkspaceImportFiles
  reset?: {
    cleared: WorkspaceImportFound[]
    people_removed: number
    people_kept: number
    data_epoch: number
  }
}

export interface WorkspaceImportActions {
  check: boolean
  confirm: boolean
  retry_files: boolean
  reset: boolean
  invite: boolean
}

/** Staff the import created who can't sign in yet (single GET only). */
export interface WorkspaceImportSignIn {
  pending: number
  invited: number
  email_ready: boolean
}

export interface WorkspaceImportJob {
  id: string
  workspace_id: number
  runner: 'owner' | 'operator' | string
  state: WorkspaceImportState
  stage: WorkspaceImportStage | null
  archive: { name: string | null; size: number; sealed: boolean; stored: boolean }
  regenerate_uuids: boolean
  report: WorkspaceImportReport | null
  error: WorkspaceImportRefusal | null
  people_created: number
  files_restored: number
  created_at: string
  updated_at: string
  checked_at: string | null
  confirmed_at: string | null
  applied_at: string | null
  finished_at: string | null
  reset_at: string | null
  pruned_at: string | null
  actions: WorkspaceImportActions
  sign_in?: WorkspaceImportSignIn
}

export interface WorkspaceImportInvites {
  sent: number
  failed: number
  sign_in: WorkspaceImportSignIn | null
}

/** Body of check, confirm and retry files. */
export interface WorkspaceImportActionBody {
  password?: string
  regenerate_uuids?: boolean
}

export const workspaceImportsQueryKey = ['workspace-imports'] as const
export const workspaceImportQueryKey = (id: string) => ['workspace-import', id] as const

const base = '/workspace/imports'

export const workspaceImportService = {
  /** The workspace's imports, newest first. */
  async list(): Promise<WorkspaceImportJob[]> {
    const { data } = await apiClient.get<WorkspaceImportJob[]>(base)
    return data ?? []
  },

  /** The newest import, or null. */
  async latest(): Promise<WorkspaceImportJob | null> {
    const { data } = await apiClient.get<WorkspaceImportJob | null>(`${base}/latest`)
    return data ?? null
  },

  async get(id: string): Promise<WorkspaceImportJob> {
    const { data } = await apiClient.get<WorkspaceImportJob>(`${base}/${id}`)
    return data
  },

  /** Upload an export. Answers 202 with the job checking, or waiting in
   *  `uploaded` for the password of a sealed archive. */
  async upload(
    file: File,
    options: { password?: string; regenerateUuids?: boolean } = {},
    onProgress?: (loaded: number, total: number) => void,
  ): Promise<WorkspaceImportJob> {
    const form = new FormData()
    if (options.password) form.append('password', options.password)
    if (options.regenerateUuids) form.append('regenerate_uuids', 'true')
    form.append('archive', file)
    const { data } = await apiClient.post<WorkspaceImportJob>(base, form, {
      headers: { 'Content-Type': 'multipart/form-data' },
      // A large archive takes a while; the server spools it as it arrives.
      timeout: 0,
      onUploadProgress: onProgress ? (e) => onProgress(e.loaded, e.total ?? file.size) : undefined,
    })
    return data
  },

  /** Check the archive again: with its password, or with new ids. */
  async check(id: string, body: WorkspaceImportActionBody = {}): Promise<WorkspaceImportJob> {
    const { data } = await apiClient.post<WorkspaceImportJob>(`${base}/${id}/check`, body)
    return data
  },

  /** Import. Answers 202 with the job importing. */
  async confirm(id: string, body: WorkspaceImportActionBody = {}): Promise<WorkspaceImportJob> {
    const { data } = await apiClient.post<WorkspaceImportJob>(`${base}/${id}/confirm`, body)
    return data
  },

  async retryFiles(id: string, body: WorkspaceImportActionBody = {}): Promise<WorkspaceImportJob> {
    const { data } = await apiClient.post<WorkspaceImportJob>(`${base}/${id}/retry-files`, body)
    return data
  },

  /** Take the workspace back to untouched, or discard an import that
   *  brought nothing. */
  async reset(id: string): Promise<WorkspaceImportJob> {
    const { data } = await apiClient.post<WorkspaceImportJob>(`${base}/${id}/reset`)
    return data
  },

  /** An invitation to each staff member the import created who can't sign
   *  in yet. */
  async invite(id: string, userUuids?: string[]): Promise<WorkspaceImportInvites> {
    const { data } = await apiClient.post<WorkspaceImportInvites>(
      `${base}/${id}/invite`,
      userUuids ? { user_uuids: userUuids } : {},
    )
    return data
  },
}

export default workspaceImportService
