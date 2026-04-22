/**
 * Ticket Type Definitions
 * Canonical ticket interface matching backend contract
 */

import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions'
import type { Device } from './device'
import type { Comment, Attachment } from './comment'
import type { Project } from './project'
import type { UserInfo } from './user'

// Re-export for convenience
export type { Device, Comment, Attachment, Project }

export interface Ticket {
  id: number
  title: string
  status: TicketStatus
  priority: TicketPriority
  created: string
  modified: string
  assignee: string
  requester: string
  requester_user?: UserInfo | null
  assignee_user?: UserInfo | null
  category_id?: number | null
  closed_at?: string
  devices?: Device[]
  comments?: Comment[]
  article_content?: string
  linkedTickets?: number[]
  linked_tickets?: number[]
  projects?: Project[]
  /** Channel this ticket was opened through (email_imap, ...). Null for tickets created via the UI / API. */
  origin_channel_id?: number | null
  /** Provider name echoed from the originating channel for quick display. Matches `channels.provider`. */
  submitted_via?: string | null
}

export interface RecentTicket {
  id: number
  title: string
  status: TicketStatus
  requester: string | null
  assignee: string | null
  created_at: string
  updated_at: string
  last_viewed_at: string
  view_count: number
}
