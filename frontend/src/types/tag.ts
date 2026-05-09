/**
 * Tag type definitions. Mirrors the backend `Tag` model from
 * `backend/sync-models/...` (no manifest yet; tags aren't a
 * sync aggregate today). Workspace-scoped namespace.
 *
 * Tags are admin-curated free-form labels staff attach to
 * tickets. Sit alongside the fixed `category_id` (one per
 * ticket) as a flexible second axis.
 */

export interface Tag {
  id: number
  name: string
  /** Display colour token. Same vocabulary the workflow_state
   *  picker uses. Null means "use the neutral default". */
  color?: string | null
  description?: string | null
  created_at: string
  updated_at: string
  /** Soft-archive timestamp. Archived tags drop out of the
   *  picker but stay in the table so historical ticket→tag
   *  references keep their join target. */
  archived_at?: string | null
}

export interface NewTagPayload {
  name: string
  color?: string | null
  description?: string | null
}

export interface TagUpdatePayload {
  name?: string
  color?: string | null
  description?: string | null
}
