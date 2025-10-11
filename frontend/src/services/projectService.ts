import axios from 'axios';
import type { Project } from '@/types/project';

// Define the API base URL
const API_URL = import.meta.env.VITE_API_URL || '/api';

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
  ticketCount: project.ticket_count
});

// Project service functions
export const projectService = {
  // Get all projects
  async getProjects(): Promise<Project[]> {
    try {
      const response = await axios.get<ProjectResponse[]>(`${API_URL}/projects`);
      return response.data.map(mapProjectResponse);
    } catch (error) {
      console.error('Error fetching projects:', error);
      throw error;
    }
  },

  // Get a single project by ID
  async getProject(id: number): Promise<Project> {
    try {
      const response = await axios.get<ProjectResponse>(`${API_URL}/projects/${id}`);
      return mapProjectResponse(response.data);
    } catch (error) {
      console.error(`Error fetching project ${id}:`, error);
      throw error;
    }
  },

  // Create a new project
  async createProject(project: Omit<Project, 'id' | 'ticketCount'>): Promise<Project> {
    try {
      const request: NewProjectRequest = {
        name: project.name,
        status: project.status
      };

      // Only add description if it's provided
      if (project.description !== undefined) {
        request.description = project.description || null;
      }

      const response = await axios.post<ProjectResponse>(`${API_URL}/projects`, request);
      return mapProjectResponse(response.data);
    } catch (error) {
      console.error('Error creating project:', error);
      throw error;
    }
  },

  // Update an existing project
  async updateProject(id: number, project: Partial<Omit<Project, 'id' | 'ticketCount'>>): Promise<Project> {
    try {
      const request: UpdateProjectRequest = {
        name: project.name,
        description: project.description !== undefined ? project.description || null : undefined,
        status: project.status
      };
      
      const response = await axios.put<ProjectResponse>(`${API_URL}/projects/${id}`, request);
      return mapProjectResponse(response.data);
    } catch (error) {
      console.error(`Error updating project ${id}:`, error);
      throw error;
    }
  },

  // Delete a project
  async deleteProject(id: number): Promise<void> {
    try {
      await axios.delete(`${API_URL}/projects/${id}`);
    } catch (error) {
      console.error(`Error deleting project ${id}:`, error);
      throw error;
    }
  },

  // Get all tickets for a project
  async getProjectTickets(id: number): Promise<any[]> {
    try {
      const response = await axios.get(`${API_URL}/projects/${id}/tickets`);
      return response.data;
    } catch (error) {
      console.error(`Error fetching tickets for project ${id}:`, error);
      throw error;
    }
  },

  // Add a ticket to a project
  async addTicketToProject(projectId: number, ticketId: number): Promise<void> {
    try {
      await axios.post(`${API_URL}/projects/${projectId}/tickets/${ticketId}`);
    } catch (error) {
      console.error(`Error adding ticket ${ticketId} to project ${projectId}:`, error);
      throw error;
    }
  },

  // Remove a ticket from a project
  async removeTicketFromProject(projectId: number, ticketId: number): Promise<void> {
    try {
      await axios.delete(`${API_URL}/projects/${projectId}/tickets/${ticketId}`);
    } catch (error) {
      console.error(`Error removing ticket ${ticketId} from project ${projectId}:`, error);
      throw error;
    }
  }
};

export default projectService; 