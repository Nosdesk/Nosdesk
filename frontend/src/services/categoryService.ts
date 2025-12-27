import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import type {
  TicketCategory,
  CategoryWithVisibility,
  CreateCategoryRequest,
  UpdateCategoryRequest,
  ReorderCategoriesRequest,
  SetCategoryVisibilityRequest
} from '@/types/category';

export const categoryService = {
  // Get categories visible to the current user
  async getCategories(): Promise<TicketCategory[]> {
    try {
      const response = await apiClient.get<TicketCategory[]>('/categories');
      return response.data;
    } catch (error) {
      logger.error('Error fetching categories:', error);
      throw error;
    }
  },

  // Get all categories with visibility info (admin only)
  async getAllCategoriesAdmin(): Promise<CategoryWithVisibility[]> {
    try {
      const response = await apiClient.get<CategoryWithVisibility[]>('/admin/categories');
      return response.data;
    } catch (error) {
      logger.error('Error fetching admin categories:', error);
      throw error;
    }
  },

  // Get a single category with visibility info (admin only)
  async getCategoryAdmin(id: number): Promise<CategoryWithVisibility> {
    try {
      const response = await apiClient.get<CategoryWithVisibility>(`/admin/categories/${id}`);
      return response.data;
    } catch (error) {
      logger.error(`Error fetching category ${id}:`, error);
      throw error;
    }
  },

  // Create a new category (admin only)
  async createCategory(request: CreateCategoryRequest): Promise<CategoryWithVisibility> {
    try {
      const response = await apiClient.post<CategoryWithVisibility>('/admin/categories', request);
      return response.data;
    } catch (error) {
      logger.error('Error creating category:', error);
      throw error;
    }
  },

  // Update a category (admin only)
  async updateCategory(id: number, request: UpdateCategoryRequest): Promise<CategoryWithVisibility> {
    try {
      const response = await apiClient.put<CategoryWithVisibility>(`/admin/categories/${id}`, request);
      return response.data;
    } catch (error) {
      logger.error(`Error updating category ${id}:`, error);
      throw error;
    }
  },

  // Delete a category (soft delete, admin only)
  async deleteCategory(id: number): Promise<void> {
    try {
      await apiClient.delete(`/admin/categories/${id}`);
    } catch (error) {
      logger.error(`Error deleting category ${id}:`, error);
      throw error;
    }
  },

  // Reorder categories (admin only)
  async reorderCategories(request: ReorderCategoriesRequest): Promise<CategoryWithVisibility[]> {
    try {
      const response = await apiClient.put<CategoryWithVisibility[]>('/admin/categories/reorder', request);
      return response.data;
    } catch (error) {
      logger.error('Error reordering categories:', error);
      throw error;
    }
  },

  // Set category visibility (admin only)
  async setCategoryVisibility(id: number, request: SetCategoryVisibilityRequest): Promise<CategoryWithVisibility> {
    try {
      const response = await apiClient.put<CategoryWithVisibility>(`/admin/categories/${id}/visibility`, request);
      return response.data;
    } catch (error) {
      logger.error(`Error setting visibility for category ${id}:`, error);
      throw error;
    }
  }
};

export default categoryService;
