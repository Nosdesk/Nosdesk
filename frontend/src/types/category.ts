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
  created_at: string
  updated_at: string
  created_by?: string | null
}

export interface CategoryWithVisibility extends TicketCategory {
  visible_to_groups: Group[]
  is_public: boolean
}

export interface CreateCategoryRequest {
  name: string
  description?: string
  color?: string
  icon?: string
  visible_to_group_ids?: number[]
}

export interface UpdateCategoryRequest {
  name?: string
  description?: string
  color?: string
  icon?: string
  is_active?: boolean
  visible_to_group_ids?: number[]
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
