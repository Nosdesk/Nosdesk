/**
 * Ticket Type Definitions
 * Canonical ticket interface matching backend contract
 */

import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions'
import type { Device } from './device'
import type { Comment, Attachment } from './comment'
import type { Project } from './project'
import type { UserInfo } from './user'
import type { WorkflowState } from './workflow'

// Re-export for convenience
export type { Device, Comment, Attachment, Project }

export interface Ticket {
  id: number
  title: string
  /**
   * Legacy three-bucket status string derived from the workflow state's
   * category. Continues to ship from the backend for wire-format
   * compatibility while the UI is migrated to read `workflow_state_id`
   * and the joined `workflow_state` directly.
   */
  status: TicketStatus
  /**
   * Always present on backend responses post-Phase-1; optional on the
   * type until input/create flows have been migrated to specify it
   * directly. The backend defaults missing values to the workspace
   * default state.
   */
  workflow_state_id?: number
  /** Joined workflow state, present on detail responses. */
  workflow_state?: WorkflowState | null
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
  /// `Project[]` is what the API ships (embedded). The detail view
  /// flattens it to `string[]` of project IDs at fetch time so the
  /// multi-select UI can mutate membership cheaply. Both shapes
  /// flow through the same property; consumers that need the
  /// embedded form re-fetch.
  projects?: Project[] | string[]
  /** Channel this ticket was opened through (email_imap, ...). Null for tickets created via the UI / API. */
  origin_channel_id?: number | null
  /** Provider name echoed from the originating channel for quick display. Matches `channels.provider`. */
  submitted_via?: string | null
  /** Calendar deadline. Null when the ticket has no committed
   * due date. ISO string at the API boundary. */
  due_date?: string | null
  /** RFC 5545 RRULE string when the ticket is part of a recurring
   * series. Closing a ticket with a rule spawns the next
   * occurrence on the backend. */
  recurrence_rule?: string | null
  recurrence_template_id?: number | null
}

export interface RecentTicket {
  id: number
  title: string
  status: TicketStatus
  workflow_state_id?: number
  requester: string | null
  assignee: string | null
  created_at: string
  updated_at: string
  last_viewed_at: string
  view_count: number
}
