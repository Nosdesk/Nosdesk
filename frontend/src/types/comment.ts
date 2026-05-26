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

/**
 * Native-first render tier set by the inbound email pipeline:
 * `text` (plaintext, rendered as a linkified pre-wrap bubble),
 * `simple` (human HTML reduced to a semantic-inline subset, rendered
 * inline), or `rich` (newsletter/layout HTML, rendered in a sandboxed
 * iframe). Null/undefined for UI-authored comments and pre-pipeline
 * rows; the renderer then falls back to per-`content_format` rendering.
 */
export type CommentRenderKind = 'text' | 'simple' | 'rich'

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
  /**
   * Backend-extracted "just the reply" from the email-rendering Pass 1
   * splitter. Present for email-derived comments; null/undefined for
   * UI-authored comments and for legacy rows ingested before the
   * splitter landed. Renderer prefers this over `content` when set
   * so the visible body is just the new reply, not the entire
   * quoted thread.
   */
  new_content?: string | null
  /**
   * Backend-extracted quoted prior thread from the same splitter.
   * Renders in a "Show quoted thread" disclosure below the reply.
   * Null when no quote boundary was detected (first-touch emails,
   * short replies) or the comment isn't email-derived.
   */
  quoted_content?: string | null
  /**
   * Native-first render tier (see `CommentRenderKind`). Set by the
   * inbound email pipeline; null/undefined for UI-authored comments and
   * pre-pipeline rows, where the renderer falls back to per-format
   * rendering.
   */
  render_kind?: CommentRenderKind | null
  /**
   * Whether the backend has an archived .eml available at
   * `/api/comments/{id}/raw.eml`. The frontend uses this flag to
   * conditionally render the "Show original message" affordance,
   * keeping the internal storage path off the wire.
   */
  has_raw_source?: boolean
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
