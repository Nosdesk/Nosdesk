/**
 * Ticket Type Definitions
 * Canonical ticket interface matching backend contract
 */

import type { TicketPriority } from '../constants/ticketOptions'
import type { Asset } from './asset'
import type { Comment, Attachment } from './comment'
import type { Project } from './project'
import type { UserInfo } from './user'
import type { WorkflowState } from './workflow'
import type { SlaPill } from './sla'

// Re-export for convenience
export type { Asset, Comment, Attachment, Project }

export interface Ticket {
  id: number
  /** Immutable per-ticket identity (never recycled like the integer
   *  id). Used to key the collaborative note doc. Optional on the type
   *  until every create/input flow carries it; always present on
   *  backend read responses. */
  uuid?: string
  title: string
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
  devices?: Asset[]
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
  /** Optional planning start for the gantt. Null means unplanned. */
  start_date?: string | null
  /** RFC 5545 RRULE string when the ticket is part of a recurring
   * series. Closing a ticket with a rule spawns the next
   * occurrence on the backend. */
  recurrence_rule?: string | null
  recurrence_template_id?: number | null
  /** UUID of the user who created the ticket. May be null for
   *  guest-portal submissions where the requester wasn't a
   *  registered user at create-time. */
  created_by?: string | null
  /** UUID of the user who closed the ticket — only meaningful
   *  alongside `closed_at`. Both populated together by the
   *  status-transition handler when a ticket lands in a terminal
   *  workflow state. */
  closed_by?: string | null
  /** Cycle membership, when the ticket belongs to one. The detail
   *  view renders a clickable pill that navigates to the cycle.
   *  Backend embeds the cycle row (name + state + ids) so the
   *  pill renders without a separate fetch — the cycles store is
   *  per-project keyed and the detail view doesn't necessarily
   *  know the cycle's project up-front. */
  cycle?: {
    id: number
    uuid: string
    project_id: number
    name: string
    state: string
  } | null
  /** SLA pill payload — same shape the list view consumes. The
   *  detail handler computes this on read so the sidebar can show
   *  the same Breached / At Risk / On Track / Paused state as
   *  the list. Null when no policy / calendar applies. */
  sla?: SlaPill | null
  /** Free-text "what fixed this?" capture. Surfaced prominently
   *  on the detail view once the ticket lands in a terminal
   *  workflow state. Distinct from the comment thread because
   *  the resolution is a structured fact, not a discussion.
   *  Backend treats empty string and null both as "no notes". */
  resolution_notes?: string | null
  /** Tag ids attached to the ticket. Frontend resolves each id
   *  to a `Tag` row via the workspace tag store. Empty array
   *  when no tags are attached. */
  tag_ids?: number[]
  /** Uuids of users watching this ticket. Drives the watch /
   *  unwatch toggle button + the watchers list in the sidebar.
   *  The current user's presence in this array flips the bell
   *  to "watching"; comment notifications fan out to every uuid
   *  in this set (in addition to requester / assignee). */
  watcher_uuids?: string[]
  /** When this ticket was merged into another, the destination's id;
   *  null when the ticket is not a merge source. Set together with
   *  `merged_at` / `merged_by_user_uuid`. A non-null value makes the
   *  ticket terminal: read-only UI, and future channel replies reroute
   *  to the destination. */
  merged_into_ticket_id?: number | null
  merged_at?: string | null
  merged_by_user_uuid?: string | null
  merge_reason?: string | null
  /** True when the ticket opened from inbound mail the provider flagged as
   *  spam. Opens flagged + low-priority (never dropped); cleared via "not
   *  spam". Read on every queue row to render the badge. */
  spam_suspected?: boolean
}

/** One merge that consumed sources into a ticket, from the
 *  `ticket.merged` activity event. */
export interface TicketMergeEvent {
  event_id: number
  merged_at: string
  merged_by_user_uuid: string | null
  merged_by_name: string | null
  source_ticket_ids: number[]
  reason: string | null
  comments_moved: number
  merge_marker_comment_id: number | null
}

/** Merge history for a ticket, from both directions. */
export interface MergeHistory {
  merged_into: {
    destination_id: number
    merged_at: string | null
    merged_by: string | null
    reason: string | null
  } | null
  merge_events: TicketMergeEvent[]
}

/** Response body of POST /api/tickets/merge. */
export interface MergeResponse {
  merge_event_id: number
  destination_ticket: Ticket
  merged_sources: Ticket[]
  comments_moved: number
  channel_messages_rerouted: number
  watchers_added_to_destination: number
  merge_marker_comment_id: number
}

export interface RecentTicket {
  id: number
  title: string
  workflow_state_id?: number
  requester: string | null
  assignee: string | null
  created_at: string
  updated_at: string
  last_viewed_at: string
  view_count: number
}
