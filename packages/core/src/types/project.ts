/**
 * Project Type Definitions
 * Canonical project interface
 */

export type ProjectStatus = 'active' | 'completed' | 'archived'

export interface Project {
  id: number
  name: string
  description?: string | null
  status: ProjectStatus
  created_at: string
  updated_at: string
  ticket_count?: number
}
