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

/**
 * What the bytes in `Comment.content` are, as declared by the writer
 * (the ProseMirror editor for staff replies, the IMAP ingest pipeline
 * for inbound email, etc). The backend uses this on outbound relay to
 * pick HTML vs plaintext for the wire; the frontend will use it in a
 * later phase to render inbound HTML emails in a sandboxed frame
 * instead of running them through the Markdown parser.
 */
export type CommentContentFormat = 'html' | 'markdown' | 'plaintext'

export interface Comment {
  id: number
  content: string
  /** Format of `content` — see `CommentContentFormat`. */
  content_format?: CommentContentFormat
  user_uuid: string
  created_at: string
  ticket_id: number
  /** True = tech-to-tech note. Never shown to requesters; never relayed back through the originating channel. */
  is_internal?: boolean
  /** Set when the comment originated from a channel (email Message-ID, Slack thread_ts, etc). */
  channel_metadata?: { forwarded_by_user_uuid?: string; [key: string]: unknown } | null
  /** Sender's external address (email for IMAP, equivalent identifier
   *  for chat channels). Sourced from `channel_messages.from_address`;
   *  `undefined` for comments authored through the helpdesk UI. */
  from_address?: string
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
