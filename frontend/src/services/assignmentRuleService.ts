import apiClient from './apiConfig'
import { logger } from '@/utils/logger'
import type {
  AssignmentRuleWithDetails,
  CreateAssignmentRuleRequest,
  UpdateAssignmentRuleRequest,
  ReorderRulesRequest,
  PreviewAssignmentRequest,
  PreviewAssignmentResponse,
  AssignmentLog
} from '@/types/assignmentRule'

export const assignmentRuleService = {
  // Get all assignment rules with details (admin only)
  async getAllRules(): Promise<AssignmentRuleWithDetails[]> {
    try {
      const response = await apiClient.get<AssignmentRuleWithDetails[]>('/admin/assignment-rules')
      return response.data
    } catch (error) {
      logger.error('Error fetching assignment rules:', error)
      throw error
    }
  },

  // Get a single rule by ID (admin only)
  async getRule(id: number): Promise<AssignmentRuleWithDetails> {
    try {
      const response = await apiClient.get<AssignmentRuleWithDetails>(`/admin/assignment-rules/${id}`)
      return response.data
    } catch (error) {
      logger.error(`Error fetching assignment rule ${id}:`, error)
      throw error
    }
  },

  // Create a new assignment rule (admin only)
  async createRule(request: CreateAssignmentRuleRequest): Promise<AssignmentRuleWithDetails> {
    try {
      const response = await apiClient.post<AssignmentRuleWithDetails>('/admin/assignment-rules', request)
      return response.data
    } catch (error) {
      logger.error('Error creating assignment rule:', error)
      throw error
    }
  },

  // Update an assignment rule (admin only)
  async updateRule(id: number, request: UpdateAssignmentRuleRequest): Promise<AssignmentRuleWithDetails> {
    try {
      const response = await apiClient.patch<AssignmentRuleWithDetails>(`/admin/assignment-rules/${id}`, request)
      return response.data
    } catch (error) {
      logger.error(`Error updating assignment rule ${id}:`, error)
      throw error
    }
  },

  // Delete an assignment rule (admin only)
  async deleteRule(id: number): Promise<void> {
    try {
      await apiClient.delete(`/admin/assignment-rules/${id}`)
    } catch (error) {
      logger.error(`Error deleting assignment rule ${id}:`, error)
      throw error
    }
  },

  // Reorder rules by priority (admin only)
  async reorderRules(request: ReorderRulesRequest): Promise<AssignmentRuleWithDetails[]> {
    try {
      const response = await apiClient.put<AssignmentRuleWithDetails[]>('/admin/assignment-rules/reorder', request)
      return response.data
    } catch (error) {
      logger.error('Error reordering assignment rules:', error)
      throw error
    }
  },

  // Preview what assignment would happen for a ticket (admin only)
  async previewAssignment(request: PreviewAssignmentRequest): Promise<PreviewAssignmentResponse> {
    try {
      const response = await apiClient.post<PreviewAssignmentResponse>('/admin/assignment-rules/preview', request)
      return response.data
    } catch (error) {
      logger.error('Error previewing assignment:', error)
      throw error
    }
  },

  // Get recent assignment logs (admin only)
  async getAssignmentLogs(): Promise<AssignmentLog[]> {
    try {
      const response = await apiClient.get<AssignmentLog[]>('/admin/assignment-rules/logs')
      return response.data
    } catch (error) {
      logger.error('Error fetching assignment logs:', error)
      throw error
    }
  }
}

export default assignmentRuleService
