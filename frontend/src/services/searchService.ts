import apiClient from './apiConfig';
import type {
  SearchParams,
  SearchResponse,
  IndexStats,
  RebuildResponse,
} from '@/types/search';

/**
 * Search service for full-text search across the application
 */
export const searchService = {
  /**
   * Execute a search query
   * @param params Search parameters
   * @returns Search results
   */
  async search(params: SearchParams): Promise<SearchResponse> {
    const queryParams = new URLSearchParams();
    queryParams.set('q', params.q);

    if (params.limit !== undefined) {
      queryParams.set('limit', params.limit.toString());
    }

    if (params.types) {
      queryParams.set('types', params.types);
    }

    const response = await apiClient.get<SearchResponse>(
      `/search?${queryParams.toString()}`
    );
    return response.data;
  },

  /**
   * Rebuild the search index (admin only)
   * @returns Rebuild statistics
   */
  async rebuildIndex(): Promise<RebuildResponse> {
    const response = await apiClient.post<RebuildResponse>('/search/rebuild');
    return response.data;
  },

  /**
   * Get search index statistics (admin only)
   * @returns Index statistics
   */
  async getStats(): Promise<IndexStats> {
    const response = await apiClient.get<IndexStats>('/search/stats');
    return response.data;
  },
};

export default searchService;
