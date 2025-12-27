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
  transcription?: string
}

export interface Comment {
  id: number
  content: string
  user_uuid: string
  created_at: string
  ticket_id: number
  attachments?: Attachment[]
  user?: UserInfo
}

export interface CommentWithAttachments {
  comment: Comment
  attachments: Attachment[]
  user?: UserInfo
}
