/**
 * Ticket Type Definitions
 * Canonical ticket interface matching backend contract
 */

import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions'
import type { Device } from './device'
import type { Comment } from './comment'
import type { Project } from './project'
import type { UserInfo } from './user'

// Re-export for convenience
export type { Device, Comment, Project }

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
  closed_at?: string
  devices?: Device[]
  comments?: Comment[]
  article_content?: string
  linkedTickets?: number[]
  linked_tickets?: number[]
  projects?: Project[]
}
