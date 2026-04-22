/**
 * Comment and Attachment Types
 * Canonical definitions for ticket comments and file attachments
 */

import type { UserInfo } from './user'

export interface Attachment {
  id: number
  url: string
  name: string
  comment_id: number
  file_size?: number
  mime_type?: string
  transcription?: string
  thumbnail_url?: string
  created_at?: string
}

export interface Comment {
  id: number
  content: string
  user_uuid: string
  created_at: string
  ticket_id: number
  /** True = tech-to-tech note. Never shown to requesters; never relayed back through the originating channel. */
  is_internal?: boolean
  /** Set when the comment originated from a channel (email Message-ID, Slack thread_ts, etc). */
  channel_metadata?: Record<string, unknown> | null
  attachments?: Attachment[]
  user?: UserInfo
}

/**
 * Comment with attachments - flat structure used in ticket detail views
 * Extends Comment with a camelCase date alias for UI convenience
 */
export interface CommentWithAttachments extends Comment {
  /** CamelCase alias for created_at, used in UI components */
  createdAt?: string
}
