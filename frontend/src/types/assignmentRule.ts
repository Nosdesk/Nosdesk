import type { UserInfo } from './user'
import type { Group } from './group'
import type { TicketCategory } from './ticket'

// Assignment method types
export type AssignmentMethod = 'direct_user' | 'group_round_robin' | 'group_random' | 'group_queue'

// Assignment trigger types
export type AssignmentTrigger = 'ticket_created' | 'category_changed'

// Core assignment rule
export interface AssignmentRule {
  id: number
  uuid: string
  name: string
  description: string | null
  priority: number
  is_active: boolean
  method: AssignmentMethod
  target_user_uuid: string | null
  target_group_id: number | null
  trigger_on_create: boolean
  trigger_on_category_change: boolean
  category_id: number | null
  conditions: Record<string, unknown> | null
  created_at: string
  updated_at: string
  created_by: string | null
}

// Assignment rule state (round-robin tracking)
export interface AssignmentRuleState {
  rule_id: number
  last_assigned_index: number
  total_assignments: number
  last_assigned_at: string | null
  last_assigned_user_uuid: string | null
}

// Assignment rule with related data
export interface AssignmentRuleWithDetails extends AssignmentRule {
  target_user: UserInfo | null
  target_group: Group | null
  category: TicketCategory | null
  state: AssignmentRuleState | null
}

// Create rule request
export interface CreateAssignmentRuleRequest {
  name: string
  description?: string
  priority?: number
  is_active?: boolean
  method: AssignmentMethod
  target_user_uuid?: string
  target_group_id?: number
  trigger_on_create?: boolean
  trigger_on_category_change?: boolean
  category_id?: number
  conditions?: Record<string, unknown>
}

// Update rule request
export interface UpdateAssignmentRuleRequest {
  name?: string
  description?: string
  priority?: number
  is_active?: boolean
  method?: AssignmentMethod
  target_user_uuid?: string | null
  target_group_id?: number | null
  trigger_on_create?: boolean
  trigger_on_category_change?: boolean
  category_id?: number | null
  conditions?: Record<string, unknown>
}

// Reorder request
export interface RuleOrder {
  id: number
  priority: number
}

export interface ReorderRulesRequest {
  orders: RuleOrder[]
}

// Preview assignment request/response
export interface PreviewAssignmentRequest {
  ticket_id: number
  trigger: AssignmentTrigger
}

export interface PreviewAssignmentResponse {
  would_assign: boolean
  rule_id: number | null
  rule_name: string | null
  assigned_user_uuid: string | null
  method: string | null
  message: string
}

// Assignment log entry
export interface AssignmentLog {
  id: number
  ticket_id: number
  rule_id: number | null
  trigger_type: string
  previous_assignee_uuid: string | null
  new_assignee_uuid: string | null
  method: AssignmentMethod
  context: Record<string, unknown> | null
  assigned_at: string
}

// Helper for method display names
export const methodDisplayNames: Record<AssignmentMethod, string> = {
  direct_user: 'Direct User',
  group_round_robin: 'Round-Robin (Group)',
  group_random: 'Random (Group)',
  group_queue: 'Group Queue'
}

// Helper for method descriptions
export const methodDescriptions: Record<AssignmentMethod, string> = {
  direct_user: 'Assign directly to a specific user',
  group_round_robin: 'Rotate assignment among group members evenly',
  group_random: 'Randomly select a group member for each ticket',
  group_queue: 'Assign to group queue (users claim tickets)'
}
