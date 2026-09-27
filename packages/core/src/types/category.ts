/**
 * Ticket Category Type Definitions
 * Category management interfaces
 */

import type { Group } from './group'

export interface TicketCategory {
  id: number
  uuid: string
  name: string
  description?: string | null
  color?: string | null
  icon?: string | null
  display_order: number
  is_active: boolean
  /** Offered to requesters as a request type. */
  requester_visible: boolean
  /** Requests of this type wait for approval before they're fulfilled. */
  approval_required: boolean
  approval_rule: ApprovalRule
  /** The requester's manager is an approver. */
  approval_by_manager: boolean
  created_at: string
  updated_at: string
  created_by?: string | null
}

/** `any`: one approval is enough; `all`: every approver must approve. */
export type ApprovalRule = 'any' | 'all'

/** A named approver of a request type. */
export interface CategoryApprover {
  uuid: string
  name: string
}

export interface CategoryWithVisibility extends TicketCategory {
  visible_to_groups: Group[]
  is_public: boolean
  approvers: CategoryApprover[]
}

/** Approval fields shared by create and update requests. */
export interface CategoryApprovalRequest {
  approval_required?: boolean
  approval_rule?: ApprovalRule
  approval_by_manager?: boolean
  approver_uuids?: string[]
}

export interface CreateCategoryRequest extends CategoryApprovalRequest {
  name: string
  description?: string
  color?: string
  icon?: string
  visible_to_group_ids?: number[]
  requester_visible?: boolean
}

export interface UpdateCategoryRequest extends CategoryApprovalRequest {
  name?: string
  description?: string
  color?: string
  icon?: string
  is_active?: boolean
  visible_to_group_ids?: number[]
  requester_visible?: boolean
}

export interface CategoryOrder {
  id: number
  display_order: number
}

export interface ReorderCategoriesRequest {
  orders: CategoryOrder[]
}

export interface SetCategoryVisibilityRequest {
  group_ids: number[]
}
