import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import type { Project } from '@/types/project';

// Define the types for API responses
interface ProjectResponse {
  id: number;
  name: string;
  description?: string | null;
  status: 'active' | 'completed' | 'archived';
  created_at: string;
  updated_at: string;
  ticket_count: number;
}

interface NewProjectRequest {
  name: string;
  description?: string | null;
  status: 'active' | 'completed' | 'archived';
}

interface UpdateProjectRequest {
  name?: string;
  description?: string | null;
  status?: 'active' | 'completed' | 'archived';
}

// Convert API response to frontend Project type
const mapProjectResponse = (project: ProjectResponse): Project => ({
  id: project.id,
  name: project.name,
  description: project.description || undefined,
  status: project.status,
  created_at: project.created_at,
  updated_at: project.updated_at,
  ticket_count: project.ticket_count
});

// Project service functions
export const projectService = {
  // Get all projects
  async getProjects(): Promise<Project[]> {
    try {
      const response = await apiClient.get<ProjectResponse[]>(`/projects`);
      return response.data.map(mapProjectResponse);
    } catch (error) {
      logger.error('Error fetching projects:', error);
      throw error;
    }
  },

  // Get a single project by ID
  async getProject(id: number): Promise<Project> {
    try {
      const response = await apiClient.get<ProjectResponse>(`/projects/${id}`);
      return mapProjectResponse(response.data);
    } catch (error) {
      logger.error(`Error fetching project ${id}:`, error);
      throw error;
    }
  },

  // Create a new project
  async createProject(project: Omit<Project, 'id' | 'ticket_count'>): Promise<Project> {
    try {
      const request: NewProjectRequest = {
        name: project.name,
        status: project.status
      };

      // Only add description if it's provided
      if (project.description !== undefined) {
        request.description = project.description || null;
      }

      const response = await apiClient.post<ProjectResponse>(`/projects`, request);
      return mapProjectResponse(response.data);
    } catch (error) {
      logger.error('Error creating project:', error);
      throw error;
    }
  },

  // Update an existing project
  async updateProject(id: number, project: Partial<Omit<Project, 'id' | 'ticket_count'>>): Promise<Project> {
    try {
      const request: UpdateProjectRequest = {
        name: project.name,
        description: project.description !== undefined ? project.description || null : undefined,
        status: project.status
      };
      
      const response = await apiClient.put<ProjectResponse>(`/projects/${id}`, request);
      return mapProjectResponse(response.data);
    } catch (error) {
      logger.error(`Error updating project ${id}:`, error);
      throw error;
    }
  },

  // Delete a project
  async deleteProject(id: number): Promise<void> {
    try {
      await apiClient.delete(`/projects/${id}`);
    } catch (error) {
      logger.error(`Error deleting project ${id}:`, error);
      throw error;
    }
  },

  // Get all tickets for a project
  async getProjectTickets(id: number): Promise<any[]> {
    try {
      const response = await apiClient.get(`/projects/${id}/tickets`);
      return response.data;
    } catch (error) {
      logger.error(`Error fetching tickets for project ${id}:`, error);
      throw error;
    }
  },

  // Add a ticket to a project
  async addTicketToProject(projectId: number, ticketId: number): Promise<void> {
    try {
      await apiClient.post(`/projects/${projectId}/tickets/${ticketId}`);
    } catch (error) {
      logger.error(`Error adding ticket ${ticketId} to project ${projectId}:`, error);
      throw error;
    }
  },

  // Remove a ticket from a project
  async removeTicketFromProject(projectId: number, ticketId: number): Promise<void> {
    try {
      await apiClient.delete(`/projects/${projectId}/tickets/${ticketId}`);
    } catch (error) {
      logger.error(`Error removing ticket ${ticketId} from project ${projectId}:`, error);
      throw error;
    }
  },

  // Update the order of tickets within a project
  async updateTicketOrder(projectId: number, ticketIds: number[]): Promise<void> {
    try {
      await apiClient.put(`/projects/${projectId}/tickets/order`, { ticket_ids: ticketIds });
    } catch (error) {
      logger.error(`Error updating ticket order for project ${projectId}:`, error);
      throw error;
    }
  }
};

export default projectService; 